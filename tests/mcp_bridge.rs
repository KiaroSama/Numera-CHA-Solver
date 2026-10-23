use axum::{
    Router,
    body::{Body, to_bytes},
    extract::{Request, State},
    http::{Method, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
};
use numera_cha_solver::{
    config::Settings,
    core::contracts::Error,
    engines::ocr::OCREngine,
    mcp::{tools::Tools, transport},
    review::ReviewStore,
    sdk::Numera,
};
use rmcp::{
    ServiceError, ServiceExt,
    model::{CallToolRequestParams, ClientCapabilities, ClientConfig, Implementation},
    transport::{
        StreamableHttpClientTransport, streamable_http_client::StreamableHttpClientTransportConfig,
    },
};
use serde_json::{Value, json};
use std::{
    path::Path,
    process::{ExitStatus, Stdio},
    sync::{Arc, Condvar, Mutex},
    time::Duration,
};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    process::{Child, ChildStdin, ChildStdout, Command},
    sync::mpsc,
    task::JoinHandle,
    time::timeout,
};
use tokio_util::sync::CancellationToken;

#[path = "mcp_bridge/direct.rs"]
mod direct;

type Check<T> = Result<T, String>;
const IO: Duration = Duration::from_secs(5);
const EXIT: Duration = Duration::from_secs(8);
const TOKEN: &str = "owned-bridge-synthetic-credential";
const OUTPUT_LIMIT: usize = 64 * 1024;

fn require(condition: bool, message: &str) -> Check<()> {
    condition.then_some(()).ok_or_else(|| message.to_owned())
}
fn root() -> Check<tempfile::TempDir> {
    let parent = Path::new(env!("CARGO_MANIFEST_DIR")).join(".ci-work/test-fixtures");
    std::fs::create_dir_all(&parent).map_err(|e| e.to_string())?;
    tempfile::Builder::new()
        .prefix("bridge fixture ")
        .tempdir_in(parent)
        .map_err(|e| e.to_string())
}

#[derive(Clone, Default)]
struct DecodeGate(Arc<(Mutex<bool>, Condvar)>);
impl DecodeGate {
    fn release(&self) {
        let (lock, event) = &*self.0;
        *lock.lock().unwrap_or_else(|e| e.into_inner()) = true;
        event.notify_all();
    }
}
impl Drop for DecodeGate {
    fn drop(&mut self) {
        self.release();
    }
}

#[derive(Debug)]
enum WireEvent {
    Opened(String),
    Recognition(Value),
    Cancelled(Value),
    Deleted(String),
}
// Observe the real router's wire traffic; do not synthesize tool responses or cancellation.
async fn observe(
    State(events): State<mpsc::Sender<WireEvent>>,
    request: Request,
    next: Next,
) -> Response {
    let (parts, body) = request.into_parts();
    let bytes = match timeout(IO, to_bytes(body, OUTPUT_LIMIT)).await {
        Ok(Ok(bytes)) => bytes,
        _ => return StatusCode::BAD_REQUEST.into_response(),
    };
    let message: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    let bridge_init = message["method"] == "initialize"
        && message["params"]["clientInfo"]["name"] == "Numera-CHA-Solver-MCP-bridge";
    if message["method"] == "tools/call"
        && message["params"]["name"] == "recognize"
        && message["params"]["arguments"]["asset_base64"] == "AQ=="
    {
        let _ = events.try_send(WireEvent::Recognition(message["id"].clone()));
    }
    let deleted = if parts.method == Method::DELETE {
        parts
            .headers
            .get("mcp-session-id")
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned)
    } else {
        None
    };
    let response = next
        .run(Request::from_parts(parts, Body::from(bytes)))
        .await;
    if response.status().is_success() {
        if bridge_init
            && let Some(id) = response
                .headers()
                .get("mcp-session-id")
                .and_then(|v| v.to_str().ok())
        {
            let _ = events.try_send(WireEvent::Opened(id.to_owned()));
        }
        if message["method"] == "notifications/cancelled" {
            let _ = events.try_send(WireEvent::Cancelled(message["params"]["requestId"].clone()));
        }
        if let Some(id) = deleted {
            let _ = events.try_send(WireEvent::Deleted(id));
        }
    }
    response
}
async fn wire_event(
    events: &mut mpsc::Receiver<WireEvent>,
    matches: impl Fn(&WireEvent) -> bool,
) -> Check<WireEvent> {
    timeout(IO, async {
        for _ in 0..16 {
            let value = events.recv().await.ok_or("Wire observer closed.")?;
            if matches(&value) {
                return Ok(value);
            }
        }
        Err("Expected wire event was not observed.".to_owned())
    })
    .await
    .map_err(|_| "Wire event deadline exhausted.".to_owned())?
}

struct BridgeChild {
    child: Child,
    pid: u32,
    input: Option<ChildStdin>,
    output: BufReader<ChildStdout>,
    stderr: Option<JoinHandle<std::io::Result<Vec<u8>>>>,
    cancelled_id: Option<Value>,
    messages: usize,
}
impl BridgeChild {
    fn spawn(root: &Path, port: u16, remote: bool) -> Check<Self> {
        let mut command = Command::new(env!("CARGO_BIN_EXE_numera-cha-solver-mcp"));
        command.arg("bridge").current_dir(root).env_clear();
        if remote {
            command.arg("--remote").env("NUMERA_MCP_TOKEN", TOKEN);
        }
        for name in [
            "SystemRoot",
            "WINDIR",
            "PATH",
            "HOME",
            "USERPROFILE",
            "TEMP",
            "TMP",
        ] {
            if let Some(value) = std::env::var_os(name) {
                command.env(name, value);
            }
        }
        command
            .env("NUMERA_ROOT", root)
            .env("TEMP", root)
            .env("TMP", root)
            .env("NUMERA_DEVICE", "cpu")
            .env("NUMERA_MCP_PORT", port.to_string())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        #[cfg(windows)]
        command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        let mut child = command.spawn().map_err(|e| e.to_string())?;
        let pid = child.id().expect("newly spawned child identity");
        let input = child.stdin.take();
        let output = BufReader::new(child.stdout.take().expect("piped stdout"));
        let stderr = child.stderr.take().expect("piped stderr");
        let stderr = tokio::spawn(async move {
            let mut bytes = Vec::new();
            stderr
                .take((OUTPUT_LIMIT + 1) as u64)
                .read_to_end(&mut bytes)
                .await?;
            Ok(bytes)
        });
        Ok(Self {
            child,
            pid,
            input,
            output,
            stderr: Some(stderr),
            cancelled_id: None,
            messages: 0,
        })
    }
    async fn send(&mut self, message: Value) -> Check<()> {
        let mut bytes = serde_json::to_vec(&message).map_err(|e| e.to_string())?;
        bytes.push(b'\n');
        let input = self.input.as_mut().ok_or("Child stdin is closed.")?;
        timeout(IO, async {
            input.write_all(&bytes).await?;
            input.flush().await
        })
        .await
        .map_err(|_| "Stdio write deadline exhausted.".to_owned())?
        .map_err(|e| e.to_string())
    }
    async fn read(&mut self) -> Check<Option<Value>> {
        timeout(IO, async {
            let mut line = Vec::new();
            loop {
                let buffer = self.output.fill_buf().await.map_err(|e| e.to_string())?;
                if buffer.is_empty() {
                    require(line.is_empty(), "Incomplete JSON-RPC line at stdout EOF.")?;
                    return Ok(None);
                }
                let count = buffer
                    .iter()
                    .position(|b| *b == b'\n')
                    .map_or(buffer.len(), |i| i + 1);
                require(
                    line.len() + count <= OUTPUT_LIMIT,
                    "Stdout line exceeded its bound.",
                )?;
                line.extend_from_slice(&buffer[..count]);
                self.output.consume(count);
                if line.last() == Some(&b'\n') {
                    break;
                }
            }
            let value: Value = serde_json::from_slice(&line)
                .map_err(|_| "Bridge stdout contained non-JSON protocol output.".to_owned())?;
            require(value["jsonrpc"] == "2.0", "Stdout was not JSON-RPC2.0.")?;
            require(
                value["method"].is_string()
                    || (value.get("id").is_some()
                        && (value.get("result").is_some() ^ value.get("error").is_some())),
                "Stdout contained an invalid JSON-RPC envelope.",
            )?;
            if let Some(id) = &self.cancelled_id {
                require(
                    value.get("id") != Some(id),
                    "Cancelled stdio request received a late response.",
                )?;
            }
            self.messages += 1;
            require(
                self.messages <= 64,
                "Bridge stdout message count exceeded its bound.",
            )?;
            Ok(Some(value))
        })
        .await
        .map_err(|_| "Stdio read deadline exhausted.".to_owned())?
    }
    async fn response(&mut self, id: Value) -> Check<Value> {
        for _ in 0..16 {
            let value = self
                .read()
                .await?
                .ok_or("Bridge closed before its response.")?;
            if value.get("id").is_some() {
                require(value["id"] == id, "Unexpected stdio response identity.")?;
                return Ok(value);
            }
        }
        Err("Bridge response was not observed.".to_owned())
    }
    async fn call(&mut self, id: &str, name: &str, arguments: Value) -> Check<Value> {
        self.send(json!({"jsonrpc":"2.0","id":id,"method":"tools/call",
            "params":{"name":name,"arguments":arguments}}))
            .await?;
        self.response(json!(id)).await
    }
    async fn reap(&mut self, force: bool) -> Check<ExitStatus> {
        drop(self.input.take());
        if force && self.child.try_wait().map_err(|e| e.to_string())?.is_none() {
            self.child.start_kill().map_err(|e| e.to_string())?;
        }
        let result = timeout(EXIT, self.child.wait()).await;
        let status = match result {
            Ok(status) => status.map_err(|e| e.to_string())?,
            Err(_) => {
                self.child.start_kill().map_err(|e| e.to_string())?;
                timeout(EXIT, self.child.wait())
                    .await
                    .map_err(|_| format!("Owned bridge PID{} could not be reaped.", self.pid))?
                    .map_err(|e| e.to_string())?;
                return Err("Bridge did not exit within its orderly shutdown bound.".to_owned());
            }
        };
        require(
            self.child.id().is_none(),
            "Reaped bridge retained a live child identity.",
        )?;
        Ok(status)
    }
    async fn stderr(&mut self) -> Check<Vec<u8>> {
        let mut task = self.stderr.take().ok_or("Stderr was already collected.")?;
        let bytes = match timeout(Duration::from_secs(3), &mut task).await {
            Ok(result) => result
                .map_err(|e| e.to_string())?
                .map_err(|e| e.to_string())?,
            Err(_) => {
                task.abort();
                let _ = timeout(IO, task).await;
                return Err("Stderr collector did not join.".to_owned());
            }
        };
        require(bytes.len() <= OUTPUT_LIMIT, "Stderr exceeded its bound.")?;
        require(
            !bytes
                .windows(TOKEN.len())
                .any(|value| value == TOKEN.as_bytes()),
            "Bridge diagnostic output exposed its credential.",
        )?;
        Ok(bytes)
    }
}

async fn invalid_review_error(port: u16) -> Check<Value> {
    let client = reqwest::Client::builder()
        .no_proxy()
        .timeout(IO)
        .build()
        .map_err(|e| e.to_string())?;
    let transport = StreamableHttpClientTransport::with_client(
        client,
        StreamableHttpClientTransportConfig::with_uri(format!("http://127.0.0.1:{port}/mcp"))
            .auth_header(TOKEN.to_owned()),
    );
    let mut direct = timeout(
        IO,
        ClientConfig::new(
            ClientCapabilities::default(),
            Implementation::new("owned-error-baseline", "1"),
        )
        .serve(transport),
    )
    .await
    .map_err(|_| "Direct initialization timed out.".to_owned())?
    .map_err(|e| e.to_string())?;
    let response = timeout(
        IO,
        direct.call_tool(
            CallToolRequestParams::new("create_review").with_arguments(
                json!({"revision":"","suggestion":"owned"})
                    .as_object()
                    .unwrap()
                    .clone(),
            ),
        ),
    )
    .await;
    let closed = timeout(EXIT, direct.close()).await;
    closed
        .map_err(|_| "Direct client cleanup timed out.".to_owned())?
        .map_err(|e| e.to_string())?;
    match response.map_err(|_| "Direct invalid-review call timed out.".to_owned())? {
        Err(ServiceError::McpError(error)) => {
            serde_json::to_value(error).map_err(|e| e.to_string())
        }
        _ => Err("Invalid review did not produce its typed upstream error.".to_owned()),
    }
}
async fn exercise(
    child: &mut BridgeChild,
    port: u16,
    sdk: &Numera,
    gate: &DecodeGate,
    started: &mut mpsc::Receiver<()>,
    events: &mut mpsc::Receiver<WireEvent>,
) -> Check<()> {
    child
        .send(
            json!({"jsonrpc":"2.0","id":"initialize","method":"initialize","params":{
        "protocolVersion":"2025-11-25","capabilities":{},
        "clientInfo":{"name":"owned-stdio-test","version":"1"}}}),
        )
        .await?;
    let info = child.response(json!("initialize")).await?;
    require(
        info["result"]["serverInfo"]["name"] == "Numera-CHA-Solver-MCP",
        "Wrong bridge identity.",
    )?;
    child
        .send(json!({"jsonrpc":"2.0","method":"notifications/initialized"}))
        .await?;
    child
        .send(json!({"jsonrpc":"2.0","id":"catalog","method":"tools/list"}))
        .await?;
    let catalog = child.response(json!("catalog")).await?;
    let tools = catalog["result"]["tools"]
        .as_array()
        .ok_or("Tool catalog missing.")?;
    require(
        tools.iter().any(|tool| tool["name"] == "recognize"),
        "Recognition tool missing.",
    )?;
    require(
        !tools.iter().any(|tool| {
            tool["name"]
                .as_str()
                .is_some_and(|name| name.contains("confirm") || name.contains("submit"))
        }),
        "Human authority tool exposed.",
    )?;
    let answer = child
        .call(
            "arithmetic",
            "recognize",
            json!({"kind":"text","text":"12 * 3 = ?"}),
        )
        .await?;
    require(
        answer["result"]["structuredContent"]["answer"] == "36",
        "Arithmetic result was not36.",
    )?;
    require(
        answer["result"]["structuredContent"]["server_verified"] == false,
        "Recognition claimed acceptance.",
    )?;
    let expected = invalid_review_error(port).await?;
    require(
        expected["data"]["code"] == "invalid_review",
        "Upstream review error lost its domain code.",
    )?;
    let invalid = child
        .call(
            "invalid-review",
            "create_review",
            json!({"revision":"","suggestion":"owned"}),
        )
        .await?;
    require(
        invalid["error"] == expected,
        "Bridge changed upstream error code/message/data.",
    )?;
    let session = match wire_event(events, |value| matches!(value, WireEvent::Opened(_))).await? {
        WireEvent::Opened(id) => id,
        _ => return Err("Expected bridge session identity.".to_owned()),
    };
    child
        .send(
            json!({"jsonrpc":"2.0","id":"slow","method":"tools/call","params":{
        "name":"recognize","arguments":{"kind":"image","asset_base64":"AQ=="}}}),
        )
        .await?;
    timeout(IO, started.recv())
        .await
        .map_err(|_| "Controlled decoder never entered.".to_owned())?
        .ok_or("Controlled decoder signal closed.")?;
    let upstream_id =
        match wire_event(events, |value| matches!(value, WireEvent::Recognition(_))).await? {
            WireEvent::Recognition(id) => id,
            _ => return Err("Expected forwarded request identity.".to_owned()),
        };
    child.cancelled_id = Some(json!("slow"));
    child
        .send(json!({"jsonrpc":"2.0","method":"notifications/cancelled",
        "params":{"requestId":"slow","reason":"Owned regression cancellation."}}))
        .await?;
    wire_event(
        events,
        |value| matches!(value, WireEvent::Cancelled(id) if id == &upstream_id),
    )
    .await?;
    let busy = child
        .call("busy", "recognize", json!({"kind":"text","text":"12 * 3"}))
        .await?;
    require(
        busy["result"]["structuredContent"]["error"]["code"] == "inference_busy",
        "Cancelled decoder released admission before draining.",
    )?;
    require(
        !sdk.decode_idle(),
        "Controlled decoder was not still owned.",
    )?;
    gate.release();
    timeout(IO, async {
        while !sdk.decode_idle() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .map_err(|_| "Controlled decoder did not drain.".to_owned())?;
    let next = child
        .call(
            "successor",
            "recognize",
            json!({"kind":"text","text":"12 * 3"}),
        )
        .await?;
    require(
        next["result"]["structuredContent"]["answer"] == "36",
        "Successor admission did not recover.",
    )?;
    let status = child.reap(false).await?;
    require(
        status.success(),
        "Bridge failed orderly stdio EOF shutdown.",
    )?;
    wire_event(
        events,
        |value| matches!(value, WireEvent::Deleted(id) if id == &session),
    )
    .await?;
    require(
        child.read().await?.is_none(),
        "Unexpected protocol output after completed EOF shutdown.",
    )?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn real_stdio_bridge_forwards_errors_cancels_and_reaps() {
    let root = root().expect("isolated project fixture");
    let mut settings = Settings::new(root.path().into());
    settings.device = "cpu".into();
    settings.queue_size = 1;
    let gate = DecodeGate::default();
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
                    Duration::from_secs(20),
                    |released| !*released,
                )
                .map_err(|_| Error::new("invalid_asset", "Decoder gate failed."))?;
            drop(guard);
            if waited.timed_out() {
                return Err(Error::new("expired", "Controlled decoder ceiling."));
            }
            Err(Error::new(
                "invalid_asset",
                "Controlled decoder completion.",
            ))
        }),
    );
    let tools = Tools {
        sdk: sdk.clone(),
        reviews: ReviewStore::new(&root.path().join("reviews.sqlite3"), 60).unwrap(),
    };
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let cancel = CancellationToken::new();
    let stop = cancel.clone();
    let (events_tx, mut events) = mpsc::channel(16);
    let router: Router = transport::router(tools, TOKEN.into(), port, cancel.clone())
        .layer(middleware::from_fn_with_state(events_tx, observe));
    let mut server = tokio::spawn(async move {
        axum::serve(listener, router)
            .with_graceful_shutdown(async move { stop.cancelled().await })
            .await
    });
    let mut child = BridgeChild::spawn(root.path(), port, true);
    let result = match &mut child {
        Ok(child) => timeout(
            Duration::from_secs(30),
            exercise(child, port, &sdk, &gate, &mut started, &mut events),
        )
        .await
        .map_err(|_| "Bridge scenario exceeded30seconds.".to_owned())
        .and_then(|value| value),
        Err(error) => Err(error.clone()),
    };
    gate.release();
    let mut failures = Vec::new();
    if let Err(error) = result {
        failures.push(error);
    }
    if let Ok(child) = &mut child {
        if let Err(error) = child.reap(!failures.is_empty()).await {
            failures.push(error);
        }
        if let Err(error) = child.stderr().await {
            failures.push(error);
        }
    }
    cancel.cancel();
    let server_cleanup = match timeout(IO, &mut server).await {
        Ok(result) => result
            .map_err(|e| e.to_string())
            .and_then(|value| value.map_err(|e| e.to_string())),
        Err(_) => {
            server.abort();
            let _ = timeout(Duration::from_secs(3), server).await;
            Err("HTTP fixture did not join graceful cleanup.".to_owned())
        }
    };
    if let Err(error) = server_cleanup {
        failures.push(error);
    }
    if timeout(EXIT, sdk.close()).await.is_err() {
        failures.push("SDK cleanup timed out.".to_owned());
    }
    if !sdk.decode_idle() {
        failures.push("Owned decoder survived cleanup.".to_owned());
    }
    assert!(
        failures.is_empty(),
        "real stdio bridge contract: {}",
        failures.join("; ")
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn bridge_unavailable_service_exits_two_without_protocol_stdout() {
    let root = root().expect("isolated project fixture");
    // Reserve a loopback port without listening: no alternate service can acquire it.
    let reservation = tokio::net::TcpSocket::new_v4().unwrap();
    reservation.bind("127.0.0.1:0".parse().unwrap()).unwrap();
    let mut child =
        BridgeChild::spawn(root.path(), reservation.local_addr().unwrap().port(), true).unwrap();
    let result = timeout(Duration::from_secs(15), async {
        // Keep the captured stdin open: failure must come from unavailable HTTP, not EOF.
        let status = timeout(Duration::from_secs(12), child.child.wait())
            .await
            .map_err(|_| "Unavailable bridge did not exit while stdin stayed open.".to_owned())?
            .map_err(|e| e.to_string())?;
        require(
            status.code() == Some(2),
            "Unavailable service did not exit2.",
        )?;
        require(
            child.read().await?.is_none(),
            "Unavailable bridge wrote protocol stdout.",
        )?;
        Ok::<(), String>(())
    })
    .await
    .map_err(|_| "Unavailable bridge exceeded15seconds.".to_owned())
    .and_then(|value| value);
    let mut failures = Vec::new();
    if let Err(error) = result {
        failures.push(error);
    }
    if let Err(error) = child.reap(!failures.is_empty()).await {
        failures.push(error);
    }
    let diagnostic = child.stderr().await.and_then(|bytes| {
        let text = std::str::from_utf8(&bytes).map_err(|_| "Non-UTF8 diagnostics.".to_owned())?;
        require(
            text.contains("mcp_unavailable"),
            "Unavailable bridge omitted its safe diagnostic.",
        )
    });
    if let Err(error) = diagnostic {
        failures.push(error);
    }
    assert!(
        failures.is_empty(),
        "fail-closed bridge contract: {}",
        failures.join("; ")
    );
}
