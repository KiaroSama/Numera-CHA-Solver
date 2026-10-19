#[path = "../examples/support/webdriver_client.rs"]
mod webdriver_client;

use serde_json::{Value, json};
use std::time::Duration;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    time::timeout,
};
use webdriver_client::{Failure, RESPONSE_LIMIT, request};

const IO: Duration = Duration::from_secs(2);
type Check<T> = Result<T, String>;

struct Case {
    status: Option<u16>,
    body: Vec<u8>,
    expected: Result<Value, Failure>,
    truncated: bool,
}

async fn inspect_request(connection: &mut TcpStream) -> Check<()> {
    let mut request = Vec::new();
    let split = loop {
        let mut chunk = [0; 1024];
        let size = timeout(IO, connection.read(&mut chunk))
            .await
            .map_err(|_| "Wire request header timed out.")?
            .map_err(|_| "Wire request header could not be read.")?;
        if size == 0 {
            return Err("Client ended before its request headers.".into());
        }
        request.extend_from_slice(&chunk[..size]);
        if request.len() > 8192 {
            return Err("Unexpected wire request size.".into());
        }
        if let Some(offset) = request.windows(4).position(|part| part == b"\r\n\r\n") {
            break offset;
        }
    };
    let header = std::str::from_utf8(&request[..split])
        .map_err(|_| "Wire request headers were not UTF-8.")?;
    let mut lines = header.split("\r\n");
    if lines.next() != Some("POST /session/controlled/execute/sync HTTP/1.1") {
        return Err("Native WebDriver request path or HTTP version changed.".into());
    }
    let mut length = None;
    let mut keepalive = false;
    for line in lines {
        let (name, value) = line.split_once(':').ok_or("Malformed wire header.")?;
        if name.eq_ignore_ascii_case("connection") {
            keepalive = value.trim().eq_ignore_ascii_case("keep-alive");
        }
        if name.eq_ignore_ascii_case("content-length") {
            length = Some(
                value
                    .trim()
                    .parse::<usize>()
                    .map_err(|_| "Invalid wire content length.")?,
            );
        }
    }
    if !keepalive {
        return Err("Connection close must not reach the driver's upstream pool.".into());
    }
    let length = length
        .filter(|size| (1..=1024).contains(size))
        .ok_or("Missing or oversized wire request body.")?;
    let mut payload = request[split + 4..].to_vec();
    while payload.len() < length {
        let mut chunk = vec![0; length - payload.len()];
        let size = timeout(IO, connection.read(&mut chunk))
            .await
            .map_err(|_| "Wire request body timed out.")?
            .map_err(|_| "Wire request body could not be read.")?;
        if size == 0 {
            return Err("Client ended before its request body.".into());
        }
        payload.extend_from_slice(&chunk[..size]);
    }
    let value: Value =
        serde_json::from_slice(&payload).map_err(|_| "Wire request body was not JSON.")?;
    if value != json!({"script": "return 1", "args": []}) {
        return Err("Native WebDriver script payload changed.".into());
    }
    Ok(())
}

async fn serve(listener: &TcpListener, cases: &[Case]) -> Check<()> {
    for case in cases {
        let (mut connection, _) = timeout(IO, listener.accept())
            .await
            .map_err(|_| "Owned wire fixture accept timed out.")?
            .map_err(|_| "Owned wire fixture accept failed.")?;
        inspect_request(&mut connection).await?;
        let Some(status) = case.status else {
            continue; // The command was received; losing its response must never replay it.
        };
        let size = case.body.len() + usize::from(case.truncated);
        let header = format!(
            "HTTP/1.1 {status} Controlled\r\nContent-Length: {size}\r\nContent-Type: application/json\r\nLocation: /must-not-follow\r\n\r\n"
        );
        timeout(IO, async {
            connection.write_all(header.as_bytes()).await?;
            connection.write_all(&case.body).await
        })
        .await
        .map_err(|_| "Owned wire response write timed out.")?
        .map_err(|_| "Owned wire response write failed.")?;
        if case.truncated {
            connection
                .shutdown()
                .await
                .map_err(|_| "Truncated response could not close.")?;
        }
        let mut byte = [0; 1];
        match timeout(IO, connection.read(&mut byte)).await {
            Ok(Ok(0)) => {}
            // Rejecting an oversized response may close with TCP RST instead of FIN; both prove peer closure.
            Ok(Err(error))
                if case.expected == Err(Failure::TooLarge)
                    && matches!(
                        error.kind(),
                        std::io::ErrorKind::ConnectionReset | std::io::ErrorKind::ConnectionAborted
                    ) => {}
            Ok(Ok(_)) => return Err("Native WebDriver reused a completed command socket.".into()),
            Ok(Err(_)) => {
                return Err("Native WebDriver client socket did not close cleanly.".into());
            }
            Err(_) => {
                return Err("Native WebDriver client socket leaked after its response.".into());
            }
        }
    }
    Ok(())
}

#[tokio::test]
async fn actual_webdriver_transport_is_bounded_utf8_keepalive_closed_and_never_replayed() {
    assert_eq!(
        webdriver_client::identifier("native.node/part α"),
        Ok("native.node%2Fpart%20%CE%B1".into())
    );
    for invalid in ["", ".", "..", "id\ncontrol"] {
        assert_eq!(
            webdriver_client::identifier(invalid),
            Err(Failure::InvalidEnvelope)
        );
    }
    let diagnostic = webdriver_client::session_failure(
        500,
        br#"{"value":{"error":"session not created","message":"Chrome exited; token=controlled-private https://controlled.invalid/path","stacktrace":"excluded"}}"#,
    );
    assert!(diagnostic.contains("HTTP 500: session not created: Chrome exited"));
    assert!(!diagnostic.contains("controlled-private"));
    assert!(!diagnostic.contains("controlled.invalid"));
    assert!(!diagnostic.contains("excluded"));
    assert!(
        webdriver_client::session_failure(500, b"not-json").contains("No W3C startup diagnostic")
    );
    let oversized = serde_json::to_vec(&json!({"value":{"message":"x".repeat(8192)}})).unwrap();
    assert_eq!(
        webdriver_client::session_failure(500, &oversized).len(),
        4096
    );
    let answer = json!({"answer": "فارسی"});
    let cases = vec![
        Case {
            status: Some(200),
            body: serde_json::to_vec(&json!({"value": answer})).unwrap(),
            expected: Ok(answer),
            truncated: false,
        },
        Case {
            status: Some(503),
            body: br#"{"value":null}"#.to_vec(),
            expected: Err(Failure::Http),
            truncated: false,
        },
        Case {
            status: Some(200),
            body: br#"{"value":{"error":"controlled"}}"#.to_vec(),
            expected: Err(Failure::Command),
            truncated: false,
        },
        Case {
            status: Some(200),
            body: vec![b'x'; RESPONSE_LIMIT + 1],
            expected: Err(Failure::TooLarge),
            truncated: false,
        },
        Case {
            status: Some(200),
            body: b"not-json".to_vec(),
            expected: Err(Failure::MalformedJson),
            truncated: false,
        },
        Case {
            status: Some(200),
            body: br#"{"not_value":null}"#.to_vec(),
            expected: Err(Failure::InvalidEnvelope),
            truncated: false,
        },
        Case {
            status: Some(200),
            body: vec![0xff],
            expected: Err(Failure::MalformedJson),
            truncated: false,
        },
        Case {
            status: Some(303),
            body: br#"{"value":null}"#.to_vec(),
            expected: Err(Failure::Http),
            truncated: false,
        },
        Case {
            status: Some(200),
            body: br#"{"value":null}"#.to_vec(),
            expected: Err(Failure::Transport),
            truncated: true,
        },
        Case {
            status: None,
            body: Vec::new(),
            expected: Err(Failure::Transport),
            truncated: false,
        },
    ];
    let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .await
        .unwrap();
    let port = listener.local_addr().unwrap().port();
    let payload = json!({"script": "return 1", "args": []});
    assert_eq!(
        request(port, "POST", "/unreachable", Some(&payload), Duration::ZERO).await,
        Err(Failure::Deadline)
    );
    // Both futures live in this task: cancellation drops sockets, without a detached fixture thread.
    let (server, ()) = timeout(Duration::from_secs(25), async {
        tokio::join!(serve(&listener, &cases), async {
            for (index, case) in cases.iter().enumerate() {
                let actual = request(
                    port,
                    "POST",
                    "/session/controlled/execute/sync",
                    Some(&payload),
                    IO,
                )
                .await;
                assert_eq!(actual, case.expected, "Native wire case {index} failed.");
            }
        })
    })
    .await
    .expect("Native WebDriver wire fixture exceeded its whole-test deadline.");
    server.expect("Owned wire fixture contract failed.");
    // Observe the listening socket, not a blind delay, for a late replay after the lost response.
    assert!(
        timeout(Duration::from_millis(100), listener.accept())
            .await
            .is_err(),
        "Native WebDriver retried or redirected an uncertain command."
    );
}
