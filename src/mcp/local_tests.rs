use super::*;
use crate::engines::ocr::OCREngine;
use serde_json::{Value, json};
use std::sync::{Arc, Condvar, Mutex};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    sync::mpsc,
};

const IO: Duration = Duration::from_secs(5);

async fn send(writer: &mut tokio::io::WriteHalf<tokio::io::DuplexStream>, value: Value) {
    let mut bytes = serde_json::to_vec(&value).unwrap();
    bytes.push(b'\n');
    timeout(IO, writer.write_all(&bytes))
        .await
        .unwrap()
        .unwrap();
}
async fn response(
    reader: &mut BufReader<tokio::io::ReadHalf<tokio::io::DuplexStream>>,
    id: &str,
) -> Value {
    timeout(IO, async {
        for _ in 0..8 {
            let mut line = String::new();
            assert!(reader.read_line(&mut line).await.unwrap() > 0);
            assert!(line.len() < 64 * 1024);
            let value: Value = serde_json::from_str(&line).unwrap();
            if value.get("id").is_some() {
                assert_eq!(value["id"], id);
                return value;
            }
        }
        panic!("Expected direct response was not observed.");
    })
    .await
    .unwrap()
}
struct Gate(Arc<(Mutex<bool>, Condvar)>);
impl Drop for Gate {
    fn drop(&mut self) {
        let (lock, event) = &*self.0;
        *lock.lock().unwrap_or_else(|e| e.into_inner()) = true;
        event.notify_all();
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn direct_cancellation_retains_admission_and_eof_drains_actual_decoder() {
    let root = tempfile::tempdir().unwrap();
    let mut settings = Settings::new(root.path().into());
    settings.device = "cpu".into();
    settings.queue_size = 1;
    let gate = Gate(Arc::new((Mutex::new(false), Condvar::new())));
    let barrier = gate.0.clone();
    let (entered, mut started) = mpsc::channel(1);
    let sdk = Numera::with_decoder(
        settings.clone(),
        OCREngine::new(settings),
        Arc::new(move |_, _| {
            let _ = entered.try_send(());
            let (lock, event) = &*barrier;
            let (guard, waited) = event
                .wait_timeout_while(
                    lock.lock().unwrap_or_else(|e| e.into_inner()),
                    Duration::from_secs(15),
                    |released| !*released,
                )
                .map_err(|_| Error::new("invalid_asset", "Owned decoder gate failed."))?;
            drop(guard);
            if waited.timed_out() {
                return Err(Error::new("expired", "Owned decoder gate ceiling."));
            }
            Err(Error::new("invalid_asset", "Owned decoder completed."))
        }),
    );
    let tools = Tools {
        sdk: sdk.clone(),
        reviews: ReviewStore::new(&root.path().join("reviews.sqlite3"), 60).unwrap(),
    };
    let (client, server) = tokio::io::duplex(64 * 1024);
    let (server_read, server_write) = tokio::io::split(server);
    let mut runtime = tokio::spawn(run_transport(tools, (server_read, server_write)));
    let (read, mut write) = tokio::io::split(client);
    let mut read = BufReader::new(read);
    let running = runtime.abort_handle();
    let observed = sdk.clone();
    let mut scenario = tokio::spawn(async move {
        send(&mut write, json!({"jsonrpc":"2.0","id":"init","method":"initialize","params":{
            "protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"owned-drain-test","version":"1"}}})).await;
        assert!(response(&mut read, "init").await.get("result").is_some());
        send(
            &mut write,
            json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
        )
        .await;
        send(
            &mut write,
            json!({"jsonrpc":"2.0","id":"slow","method":"tools/call","params":{
            "name":"recognize","arguments":{"kind":"image","asset_base64":"AQ=="}}}),
        )
        .await;
        timeout(IO, started.recv()).await.unwrap().unwrap();
        send(&mut write, json!({"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":"slow"}})).await;
        send(
            &mut write,
            json!({"jsonrpc":"2.0","id":"busy","method":"tools/call","params":{
            "name":"recognize","arguments":{"kind":"text","text":"1+2"}}}),
        )
        .await;
        assert_eq!(
            response(&mut read, "busy").await["result"]["structuredContent"]["error"]["code"],
            "inference_busy"
        );
        assert!(!observed.decode_idle());
        write.shutdown().await.unwrap();
        // Observe closed admission, not elapsed time, before releasing the actual blocking decoder.
        timeout(IO, async {
            loop {
                let result = observed
                    .recognize_value(json!({"kind":"text","text":"1+2"}), None)
                    .await;
                if result
                    .error
                    .as_ref()
                    .is_some_and(|error| error.code == "cancelled")
                {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        assert!(
            !running.is_finished(),
            "EOF abandoned the admitted decoder."
        );
        drop(gate);
    });
    let outcome = timeout(Duration::from_secs(12), &mut scenario).await;
    if outcome.is_err() {
        scenario.abort();
        let _ = scenario.await;
    }
    let joined = timeout(IO, &mut runtime).await;
    if joined.is_err() {
        runtime.abort();
        let _ = runtime.await;
    }
    assert!(
        matches!(outcome, Ok(Ok(()))),
        "Direct lifecycle scenario failed: {outcome:?}"
    );
    assert!(
        joined.unwrap().unwrap().is_ok(),
        "Direct runtime failed orderly drain."
    );
    assert!(sdk.decode_idle());
}

#[tokio::test]
async fn initialization_eof_closes_sdk_and_nested_failure_is_not_success() {
    let root = tempfile::tempdir().unwrap();
    let settings = Settings::new(root.path().into());
    let sdk = Numera::new(settings);
    let tools = Tools {
        sdk: sdk.clone(),
        reviews: ReviewStore::new(&root.path().join("reviews.sqlite3"), 60).unwrap(),
    };
    let (client, server) = tokio::io::duplex(1024);
    drop(client);
    let (read, write) = tokio::io::split(server);
    let error = timeout(IO, run_transport(tools, (read, write)))
        .await
        .unwrap()
        .unwrap_err();
    assert_eq!(error.code, "mcp_stdio_failed");
    let answer = sdk
        .recognize_value(json!({"kind":"text","text":"1+2"}), None)
        .await;
    assert_eq!(answer.error.unwrap().code, "cancelled");
    let joined = tokio::spawn(async { panic!("owned join-error fixture") })
        .await
        .unwrap_err();
    assert!(orderly(Ok(QuitReason::JoinError(joined))).is_err());
    assert!(orderly(Ok(QuitReason::Closed)).is_ok());
    assert!(orderly(Ok(QuitReason::Cancelled)).is_ok());
}
