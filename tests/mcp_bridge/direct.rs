use super::*;

async fn initialize(child: &mut BridgeChild) -> Check<()> {
    child
        .send(
            json!({"jsonrpc":"2.0","id":"init","method":"initialize","params":{
        "protocolVersion":"2025-11-25","capabilities":{},
        "clientInfo":{"name":"owned-direct-test","version":"1"}}}),
        )
        .await?;
    let info = child.response(json!("init")).await?;
    require(
        info["result"]["serverInfo"]["name"] == "Numera-CHA-Solver-MCP",
        "Direct identity differs.",
    )?;
    child
        .send(json!({"jsonrpc":"2.0","method":"notifications/initialized"}))
        .await?;
    child
        .send(json!({"jsonrpc":"2.0","id":"list","method":"tools/list"}))
        .await?;
    let catalog = child.response(json!("list")).await?;
    let tools = catalog["result"]["tools"]
        .as_array()
        .ok_or("Missing direct tools.")?;
    let mut names: Vec<_> = tools
        .iter()
        .filter_map(|tool| tool["name"].as_str())
        .collect();
    names.sort_unstable();
    require(
        names
            == [
                "cancel_review",
                "capabilities",
                "create_review",
                "doctor",
                "get_review",
                "models",
                "recognize",
            ],
        "Direct tool inventory differs.",
    )
}

async fn scenario(first: &mut BridgeChild, second: &mut BridgeChild) -> Check<String> {
    initialize(first).await?;
    initialize(second).await?;
    let answer = first
        .call(
            "answer",
            "recognize",
            json!({"kind":"text","text":"12 * 3 = ?"}),
        )
        .await?;
    require(
        answer["result"]["structuredContent"]["answer"] == "36",
        "Direct arithmetic failed.",
    )?;
    require(
        answer["result"]["structuredContent"]["server_verified"] == false,
        "Direct result claimed acceptance.",
    )?;
    let review = first
        .call(
            "create",
            "create_review",
            json!({"revision":"direct-001","suggestion":"36"}),
        )
        .await?;
    let id = review["result"]["structuredContent"]["id"]
        .as_str()
        .ok_or("Review id missing.")?
        .to_owned();
    let read = second.call("read", "get_review", json!({"id":id})).await?;
    require(
        read["result"]["structuredContent"]["state"] == "pending",
        "Concurrent client cannot read persisted review.",
    )?;
    let invalid = second
        .call(
            "invalid",
            "recognize",
            json!({"kind":"text","text":"1 + 2","unknown":true}),
        )
        .await?;
    require(
        invalid.get("error").is_some() || invalid["result"]["isError"] == true,
        "Direct schema accepted unknown field.",
    )?;
    require(first.reap(false).await?.success(), "Direct EOF failed.")?;
    let next = second
        .call("next", "recognize", json!({"kind":"text","text":"7 + 2"}))
        .await?;
    require(
        next["result"]["structuredContent"]["answer"] == "9",
        "Closing one client stopped another.",
    )?;
    Ok(id)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn direct_clients_need_no_server_or_token_and_reconnect_to_persisted_reviews() {
    let root = root().unwrap();
    let reservation = tokio::net::TcpSocket::new_v4().unwrap();
    reservation.bind("127.0.0.1:0".parse().unwrap()).unwrap();
    let port = reservation.local_addr().unwrap().port();
    let mut first = BridgeChild::spawn(root.path(), port, false).unwrap();
    let mut second = BridgeChild::spawn(root.path(), port, false).unwrap();
    let result = timeout(Duration::from_secs(25), scenario(&mut first, &mut second)).await;
    let mut failures = Vec::new();
    let id = match result {
        Ok(Ok(id)) => Some(id),
        Ok(Err(error)) => {
            failures.push(error);
            None
        }
        Err(_) => {
            failures.push("Direct scenario exceeded25seconds.".into());
            None
        }
    };
    for child in [&mut first, &mut second] {
        match child.reap(!failures.is_empty()).await {
            Ok(status) if !status.success() && failures.is_empty() => {
                failures.push(format!("Direct child exited unsuccessfully: {status}"));
            }
            Err(error) => failures.push(error),
            _ => {}
        }
        if let Err(error) = child.stderr().await {
            failures.push(error);
        }
    }
    if let Some(id) = id {
        let mut restarted = BridgeChild::spawn(root.path(), port, false).unwrap();
        let check = timeout(Duration::from_secs(15), async {
            initialize(&mut restarted).await?;
            let review = restarted
                .call("persisted", "get_review", json!({"id":id}))
                .await?;
            require(
                review["result"]["structuredContent"]["state"] == "pending",
                "Review lost after reconnect.",
            )?;
            let cancel = restarted
                .call(
                    "cancel",
                    "cancel_review",
                    json!({"id":id,"revision":"direct-001"}),
                )
                .await?;
            require(
                cancel["result"]["structuredContent"]["state"] == "cancelled",
                "Persisted cancellation failed.",
            )
        })
        .await;
        if !matches!(check, Ok(Ok(()))) {
            failures.push(format!("Reconnect failed: {check:?}"));
        }
        match restarted.reap(!failures.is_empty()).await {
            Ok(status) if !status.success() && failures.is_empty() => {
                failures.push(format!("Reconnected child exited unsuccessfully: {status}"));
            }
            Err(error) => failures.push(error),
            _ => {}
        }
        if let Err(error) = restarted.stderr().await {
            failures.push(error);
        }
    }
    assert!(
        !root.path().join("data/mcp-token.txt").exists(),
        "Direct mode created an HTTP credential."
    );
    assert!(failures.is_empty(), "Direct stdio: {}", failures.join("; "));
}
