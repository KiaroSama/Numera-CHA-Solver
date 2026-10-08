use numera_cha_solver::{
    config::Settings, core::contracts::Input, readiness::Readiness, sdk::Numera, store::TaskStore,
    tasks::TaskService,
};
use serde_json::json;
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

#[test]
fn persisted_request_digests_match_generated_python_ascii_escaping() {
    let oracle: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/parity/request-digests.json"))
            .expect("source oracle");
    for case in oracle["cases"].as_array().expect("cases") {
        let input = Input::from_value(case["input"].clone()).expect("valid observation");
        assert_eq!(
            numera_cha_solver::tasks::request_digest(&input),
            case["expected"].as_str().expect("source digest")
        );
    }
}

#[test]
fn persisted_expiry_ttl_replacement_and_missing_transition_match_source() {
    let root = tempfile::tempdir().expect("isolated task root");
    let clock = Arc::new(AtomicU64::new(1000));
    let source = clock.clone();
    let store = TaskStore::with_clock(
        &root.path().join("tasks.sqlite3"),
        60,
        Arc::new(move || source.load(Ordering::SeqCst) as f64),
    )
    .expect("open store");
    assert!(
        !store
            .transition("absent", "validating", None)
            .expect("missing false")
    );
    let task = store.create("one", "digest", 1.0).expect("create");
    let id = task["id"].as_str().expect("id");
    assert!(
        store
            .transition(id, "validating", Some(""))
            .expect("advance")
    );
    assert_eq!(
        store.get(id).expect("read")["events"][1],
        json!({"state":"validating"})
    );
    assert_eq!(
        store
            .transition(id, "queued", None)
            .expect_err("backward rejected")
            .code,
        "invalid_task"
    );
    clock.store(1002, Ordering::SeqCst);
    assert_eq!(
        store
            .create("one", "digest", 1.0)
            .expect("existing row not implicitly expired")["state"],
        "validating"
    );
    assert!(
        !store
            .transition(id, "detecting", None)
            .expect("overdue transition false")
    );
    assert_eq!(store.get(id).expect("expired task")["outcome"], "expired");
    assert!(
        !store
            .finish(id, json!({"outcome":"recognized"}))
            .expect("terminal compare and set")
    );
    clock.store(1061, Ordering::SeqCst);
    assert_eq!(
        store.get(id).expect_err("retrieval TTL").code,
        "task_missing"
    );
    assert!(store.list(50).expect("list").is_empty());
    assert_ne!(
        store
            .create("one", "new-digest", 1.0)
            .expect("expired key replaced")["id"],
        task["id"]
    );
}

#[test]
fn corrupt_persisted_result_is_not_silently_null_and_restart_never_replays() {
    let root = tempfile::tempdir().expect("isolated root");
    let path = root.path().join("tasks.sqlite3");
    let store = TaskStore::new(&path, 60).expect("store");
    let cancelled = store.create("same", "digest", 120.0).expect("task");
    let id = cancelled["id"].as_str().expect("id");
    assert_eq!(
        store.create("same", "digest", 120.0).expect("duplicate")["id"],
        cancelled["id"]
    );
    store.cancel(id).expect("cancel");
    assert!(
        !store
            .finish(id, json!({"outcome":"recognized","answer":"36"}))
            .expect("late result")
    );
    assert_eq!(
        store
            .create("same", "other", 120.0)
            .expect_err("conflict")
            .code,
        "idempotency_conflict"
    );
    let unfinished = store.create("once", "digest", 120.0).expect("unfinished");
    drop(store);
    let reopened = TaskStore::new(&path, 60).expect("restart");
    assert_eq!(
        reopened
            .get(unfinished["id"].as_str().expect("id"))
            .expect("interrupted")["result"]["error"]["code"],
        "interrupted"
    );
    // Fixture corruption is external persisted input, not a mocked store method.
    let connection = rusqlite::Connection::open(&path).expect("fixture input");
    connection
        .execute("UPDATE tasks SET result='{' WHERE id=?", [id])
        .expect("corrupt fixture");
    assert_eq!(
        reopened.get(id).expect_err("corruption reported").code,
        "store_failed"
    );
}

#[tokio::test]
async fn queued_duplicate_overflow_cancel_and_shutdown_are_terminal() {
    let root = tempfile::tempdir().expect("isolated root");
    let path = root.path().join("tasks.sqlite3");
    let mut settings = Settings::new(root.path().into());
    settings.queue_size = 1;
    let store = Arc::new(TaskStore::new(&path, 60).expect("store"));
    let service = TaskService::new_paused(Numera::new(settings), store.clone());
    let request = Input::from_value(json!({"kind":"text","text":"1+2"})).expect("request");
    let first = service.create("one", request.clone()).await.expect("first");
    assert_eq!(
        service
            .create("one", request.clone())
            .await
            .expect("duplicate")["id"],
        first["id"]
    );
    let rejected = service.create("two", request).await.expect("full terminal");
    assert_eq!(rejected["result"]["error"]["code"], "queue_full");
    assert_eq!(
        service
            .cancel(first["id"].as_str().expect("id"))
            .await
            .expect("cancel")["outcome"],
        "cancelled"
    );
    service.close().await.expect("durable task shutdown");
    assert_eq!(
        store
            .get(first["id"].as_str().expect("id"))
            .expect("stored")["outcome"],
        "cancelled"
    );
    drop(service);
    drop(store);
    let reopened = TaskStore::new(&path, 60).expect("reopen");
    assert_eq!(
        reopened
            .get(first["id"].as_str().expect("id"))
            .expect("no replay")["outcome"],
        "cancelled"
    );
}

#[tokio::test]
async fn unstarted_queue_shutdown_cancels_every_item_and_closed_service_rejects() {
    let root = tempfile::tempdir().expect("root");
    let store = Arc::new(TaskStore::new(&root.path().join("tasks.sqlite3"), 60).expect("store"));
    let service = TaskService::new_paused(
        Numera::new(Settings::new(root.path().into())),
        store.clone(),
    );
    let input = Input::from_value(json!({"kind":"text","text":"7+5"})).expect("request");
    let task = service
        .create("queued", input.clone())
        .await
        .expect("queued");
    service.close().await.expect("durable task shutdown");
    assert_eq!(
        store.get(task["id"].as_str().expect("id")).expect("stored")["outcome"],
        "cancelled"
    );
    assert_eq!(
        service
            .create("after-close", input)
            .await
            .expect_err("closed")
            .code,
        "cancelled"
    );
    service.close().await.expect("durable task shutdown");
}

#[tokio::test]
async fn readiness_is_singleflight_offloop_and_close_retains_draining_refresh() {
    use std::{
        sync::{Condvar, Mutex, atomic::AtomicUsize},
        time::Duration,
    };
    let root = tempfile::tempdir().expect("isolated root");
    let settings = Settings::new(root.path().into());
    let calls = Arc::new(AtomicUsize::new(0));
    let gate = Arc::new((Mutex::new(false), Condvar::new()));
    let (entered, mut started) = tokio::sync::mpsc::channel(1);
    let count = calls.clone();
    let barrier = gate.clone();
    let readiness = Readiness::with_verifier(
        settings.clone(),
        Arc::new(move || {
            count.fetch_add(1, Ordering::SeqCst);
            let _ = entered.try_send(());
            let (lock, signal) = &*barrier;
            let ready = lock.lock().expect("gate");
            let (_guard, status) = signal
                .wait_timeout_while(ready, Duration::from_secs(2), |ready| !*ready)
                .expect("bounded gate");
            if status.timed_out() {
                return Err(numera_cha_solver::core::contracts::Error::new(
                    "expired",
                    "Controlled verifier ceiling.",
                ));
            }
            Ok(json!({"availability":"model_missing"}))
        }),
    );
    let first = readiness.get().await;
    assert_eq!(first["refreshing"], true);
    tokio::time::timeout(Duration::from_secs(1), started.recv())
        .await
        .expect("verifier entered")
        .expect("signal");
    let sdk = Numera::new(settings);
    let result = tokio::time::timeout(
        Duration::from_secs(1),
        sdk.recognize_value(json!({"kind":"text","text":"1+2"}), None),
    )
    .await
    .expect("arithmetic responsive");
    assert_eq!(result.answer.as_deref(), Some("3"));
    for _ in 0..3 {
        assert_eq!(readiness.get().await["refreshing"], true);
    }
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let closing = readiness.clone();
    let close = tokio::spawn(async move {
        closing.close().await;
    });
    tokio::task::yield_now().await;
    assert_eq!(readiness.get().await["refreshing"], true);
    {
        let (lock, signal) = &*gate;
        *lock.lock().expect("release") = true;
        signal.notify_all();
    }
    tokio::time::timeout(Duration::from_secs(3), close)
        .await
        .expect("close drained")
        .expect("close joined");
    let final_state = readiness.get().await;
    assert_eq!(final_state["refreshing"], false);
    assert_eq!(final_state["model"]["availability"], "model_missing");
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert!(final_state["checked_at"].is_string());
    sdk.close().await;
}

#[tokio::test]
async fn shutdown_storage_failure_is_reported_instead_of_claiming_durable_cancellation() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("tasks.sqlite3");
    let store = Arc::new(TaskStore::new(&path, 60).unwrap());
    let service = TaskService::new_paused(
        Numera::new(Settings::new(root.path().into())),
        store.clone(),
    );
    let task = service
        .create(
            "queued",
            Input::from_value(json!({"kind":"text","text":"1+2"})).unwrap(),
        )
        .await
        .unwrap();
    let external = rusqlite::Connection::open(path).unwrap();
    external
        .execute(
            "UPDATE tasks SET events='{' WHERE id=?",
            [task["id"].as_str().unwrap()],
        )
        .unwrap();
    assert_eq!(service.close().await.unwrap_err().code, "store_failed");
}
