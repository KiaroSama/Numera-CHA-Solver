use numera_cha_solver::{
    api, config::Settings, core::contracts::Input, store::TaskStore, tasks::request_digest,
};
use serde_json::{Value, json};
use std::time::Duration;

#[test]
fn durable_tasks_cancel_idempotency_expiry_and_restart_without_replay() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("tasks.sqlite3");
    let store = TaskStore::new(&path, 60).unwrap();
    let task = store.create("stable", "digest", 120.0).unwrap();
    let id = task["id"].as_str().unwrap();
    assert_eq!(
        store.create("stable", "digest", 120.0).unwrap()["id"],
        task["id"]
    );
    assert_eq!(
        store.create("stable", "different", 120.0).unwrap_err().code,
        "idempotency_conflict"
    );
    assert!(store.transition(id, "recognizing", None).unwrap());
    assert!(store.transition(id, "validating", None).is_err());
    assert_eq!(store.cancel(id).unwrap()["outcome"], "cancelled");
    assert!(
        !store
            .finish(id, json!({"outcome":"recognized","answer":"3"}))
            .unwrap()
    );
    assert_eq!(store.get(id).unwrap()["outcome"], "cancelled");
    let unfinished = store.create("unfinished", "digest", 120.0).unwrap();
    drop(store);
    let reopened = TaskStore::new(&path, 60).unwrap();
    assert_eq!(
        reopened.get(unfinished["id"].as_str().unwrap()).unwrap()["result"]["error"]["code"],
        "interrupted"
    );
    assert_eq!(reopened.get(id).unwrap()["outcome"], "cancelled");
    for key in ["", &"k".repeat(129)] {
        assert!(reopened.create(key, "x", 120.0).is_err())
    }
    for budget in [0.0, 121.0, f64::NAN] {
        assert!(reopened.create("invalid", "x", budget).is_err())
    }
}

#[tokio::test]
async fn authorized_api_strict_schema_tasks_and_readiness() {
    let root = tempfile::tempdir().unwrap();
    let mut settings = Settings::new(root.path().into());
    settings.api_token = "owned-test-only".into();
    let state = api::state(settings).unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let (stop, shutdown) = tokio::sync::oneshot::channel();
    let app = api::router(state.clone());
    let server = tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(async {
                let _ = shutdown.await;
            })
            .await
            .unwrap();
    });
    let client = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(5))
        .build()
        .unwrap();
    let url = format!("http://{address}");
    let unauthorized = client.get(format!("{url}/v1/models")).send().await.unwrap();
    assert_eq!(unauthorized.status(), 401);
    let session = client
        .post(format!("{url}/v1/session"))
        .bearer_auth("owned-test-only")
        .send()
        .await
        .unwrap();
    assert_eq!(session.status(), 200);
    let cookie = session.headers()["set-cookie"].to_str().unwrap().to_owned();
    assert!(cookie.contains("HttpOnly") && cookie.contains("SameSite=strict"));
    let cookie = cookie.split(';').next().unwrap();
    assert_eq!(
        client
            .get(format!("{url}/v1/models"))
            .header("cookie", cookie)
            .send()
            .await
            .unwrap()
            .status(),
        200
    );
    let failed = client
        .post(format!("{url}/v1/recognize"))
        .header("cookie", cookie)
        .header("content-type", "application/json")
        .body("{\"kind\":\"image\",\"asset_base64\":\"!\"}")
        .send()
        .await
        .unwrap();
    assert_eq!(failed.status(), 422);
    let failed: Value = serde_json::from_slice(&failed.bytes().await.unwrap()).unwrap();
    assert!(failed["request_id"].is_string());
    let result = client
        .post(format!("{url}/v1/recognize"))
        .bearer_auth("owned-test-only")
        .header("content-type", "application/json")
        .body(json!({"kind":"text","text":"1+2"}).to_string())
        .send()
        .await
        .unwrap();
    assert_eq!(result.status(), 200);
    assert_eq!(result.headers()["x-content-type-options"], "nosniff");
    let result: Value = serde_json::from_slice(&result.bytes().await.unwrap()).unwrap();
    assert_eq!(result["answer"], "3");
    assert_eq!(result["server_verified"], false);
    for body in [
        json!({"kind":"text","text":1}),
        json!({"kind":"text","endpoint":"https://example.com"}),
        json!({"kind":"image","asset_base64":"%%%"}),
    ] {
        assert_eq!(
            client
                .post(format!("{url}/v1/recognize"))
                .bearer_auth("owned-test-only")
                .header("content-type", "application/json")
                .body(body.to_string())
                .send()
                .await
                .unwrap()
                .status(),
            422
        );
    }
    assert_eq!(
        client
            .get(format!("{url}/health/live"))
            .header("host", "outside.invalid")
            .send()
            .await
            .unwrap()
            .status(),
        403
    );
    assert_eq!(
        client
            .post(format!("{url}/v1/recognize"))
            .bearer_auth("owned-test-only")
            .header("origin", "https://outside.invalid")
            .body("{}")
            .send()
            .await
            .unwrap()
            .status(),
        403
    );
    let created = client
        .post(format!("{url}/v1/tasks"))
        .bearer_auth("owned-test-only")
        .header("idempotency-key", "api-stable")
        .header("content-type", "application/json")
        .body("{\"kind\":\"text\",\"text\":\"12*3\"}")
        .send()
        .await
        .unwrap();
    assert_eq!(created.status(), 202);
    let created: Value = serde_json::from_slice(&created.bytes().await.unwrap()).unwrap();
    let duplicate = client
        .post(format!("{url}/v1/tasks"))
        .bearer_auth("owned-test-only")
        .header("idempotency-key", "api-stable")
        .header("content-type", "application/json")
        .body("{\"kind\":\"text\",\"text\":\"12*3\"}")
        .send()
        .await
        .unwrap();
    assert_eq!(duplicate.status(), 202);
    let duplicate: Value = serde_json::from_slice(&duplicate.bytes().await.unwrap()).unwrap();
    assert_eq!(duplicate["id"], created["id"]);
    let cancelled = client
        .post(format!(
            "{url}/v1/tasks/{}/cancel",
            created["id"].as_str().unwrap()
        ))
        .bearer_auth("owned-test-only")
        .send()
        .await
        .unwrap();
    assert_eq!(cancelled.status(), 200);
    let cancelled: Value = serde_json::from_slice(&cancelled.bytes().await.unwrap()).unwrap();
    assert_eq!(cancelled["state"], "terminal");
    let task = state
        .tasks
        .create(
            "test-key",
            Input::from_value(json!({"kind":"text","text":"7+5"})).unwrap(),
        )
        .await
        .unwrap();
    let cancelled = state
        .tasks
        .cancel(task["id"].as_str().unwrap())
        .await
        .unwrap();
    assert!(matches!(
        cancelled["outcome"].as_str(),
        Some("cancelled" | "recognized")
    ));
    let ready = state.readiness.get().await;
    assert_eq!(ready["ready"], true);
    assert_eq!(ready["gpu_execution_verified"], false);
    let _ = stop.send(());
    tokio::time::timeout(Duration::from_secs(5), server)
        .await
        .unwrap()
        .unwrap();
    state.tasks.close().await.expect("durable task shutdown");
    state.readiness.close().await;
}

#[tokio::test]
async fn queued_shutdown_cancels_owned_tasks_and_terminal_result_wins() {
    let root = tempfile::tempdir().unwrap();
    let mut settings = Settings::new(root.path().into());
    settings.api_token = "owned-test-only".into();
    let state = api::state(settings).unwrap();
    let input = Input::from_value(json!({"kind":"audio"})).unwrap();
    let task = state.tasks.create("shutdown", input).await.unwrap();
    state.tasks.close().await.expect("durable task shutdown");
    let result = state.tasks.store.get(task["id"].as_str().unwrap()).unwrap();
    assert_eq!(result["state"], "terminal");
    assert!(matches!(
        result["outcome"].as_str(),
        Some("unsupported" | "cancelled")
    ));
    state.readiness.close().await;
}

#[test]
fn rollback_preserves_copied_task_schema_and_source_digest() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("fixture.sqlite3");
    let store = TaskStore::new(&path, 60).unwrap();
    let input = Input::from_value(json!({"kind":"text","text":"1+2"})).unwrap();
    let task = store
        .create("fixture-key", &request_digest(&input), 120.0)
        .unwrap();
    store
        .finish(
            task["id"].as_str().unwrap(),
            json!({"outcome":"recognized","answer":"3"}),
        )
        .unwrap();
    drop(store);
    let source = rusqlite::Connection::open(&path).unwrap();
    let columns = source
        .prepare("PRAGMA table_info(tasks)")
        .unwrap()
        .query_map([], |r| r.get::<_, String>(1))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(
        columns,
        [
            "id", "key", "digest", "created", "deadline", "expires", "state", "outcome", "result",
            "events"
        ]
    );
    let result: String = source
        .query_row(
            "SELECT result FROM tasks WHERE key='fixture-key'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&result).unwrap()["answer"],
        "3"
    );
    drop(source);
    let restored = TaskStore::new(&path, 60).unwrap();
    assert_eq!(
        restored
            .create("fixture-key", &request_digest(&input), 120.0)
            .unwrap()["id"],
        task["id"]
    );
}
