use numera_cha_solver::{config::Settings, core::contracts::Error, engines::ocr::OCREngine};
use serde_json::{Value, json};
use std::{future::Future, net::TcpListener, process::Command, sync::Arc, time::Duration};
use tokio::time::Instant;

fn fixture_engine(
    root: &std::path::Path,
    mode: &str,
    verify: Arc<dyn Fn() -> Result<Value, Error> + Send + Sync>,
) -> (OCREngine, tokio::net::TcpListener) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("isolated control port");
    listener.set_nonblocking(true).expect("nonblocking");
    let executable = std::env::split_paths(&std::env::var_os("PATH").expect("PATH"))
        .map(|directory| directory.join(if cfg!(windows) { "pwsh.exe" } else { "pwsh" }))
        .find(|path| path.is_file())
        .expect("existing PowerShell 7 executable");
    let mut command = Command::new(executable);
    command
        .args([
            "-NoLogo",
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            FIXTURE_SCRIPT,
        ])
        .env(
            "NUMERA_FIXTURE_ADDRESS",
            listener.local_addr().expect("address").to_string(),
        )
        .env("NUMERA_FIXTURE_MODE", mode)
        .env(
            "NUMERA_FIXTURE_DIAGNOSTIC",
            root.join("worker-diagnostic.txt"),
        );
    let mut settings = Settings::new(root.into());
    settings.device = "cpu".into();
    (
        OCREngine::with_runtime(settings, command, verify),
        tokio::net::TcpListener::from_std(listener).expect("control listener"),
    )
}
fn ready() -> Arc<dyn Fn() -> Result<Value, Error> + Send + Sync> {
    Arc::new(|| Ok(json!({"availability":"ready"})))
}
async fn entered(listener: &tokio::net::TcpListener) -> (tokio::net::TcpStream, u32) {
    use tokio::io::AsyncReadExt;
    let (mut stream, _) = tokio::time::timeout(Duration::from_secs(3), listener.accept())
        .await
        .expect("worker connects")
        .expect("connection");
    let pid = tokio::time::timeout(Duration::from_secs(1), stream.read_u32_le())
        .await
        .expect("pid deadline")
        .expect("worker pid");
    (stream, pid)
}
async fn wait_idle(engine: &OCREngine) {
    tokio::time::timeout(Duration::from_secs(3), async {
        while !engine.idle().await {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("owned cleanup drained");
}
#[cfg(windows)]
struct ProcessHandle(windows_sys::Win32::Foundation::HANDLE);
#[cfg(windows)]
impl ProcessHandle {
    fn open(pid: u32) -> Self {
        use windows_sys::Win32::System::Threading::{
            OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE,
        };
        // SAFETY: the PID came from the isolated worker pipe; request only wait/query rights.
        let handle = unsafe {
            OpenProcess(
                PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE,
                0,
                pid,
            )
        };
        assert!(!handle.is_null(), "owned process handle");
        Self(handle)
    }
    fn exited(&self) {
        // SAFETY: this handle remains valid until Drop and names the original process, not a recycled PID.
        assert_eq!(
            unsafe { windows_sys::Win32::System::Threading::WaitForSingleObject(self.0, 0) },
            0,
            "worker fully exited"
        );
    }
}
#[cfg(windows)]
impl Drop for ProcessHandle {
    fn drop(&mut self) {
        unsafe {
            windows_sys::Win32::Foundation::CloseHandle(self.0);
        }
    }
}

// PowerShell is an existing launcher prerequisite. Its raw streams avoid libtest's stdout banner.
// This fixture exchanges the real worker framing; it is lifecycle evidence, never model accuracy.
const FIXTURE_SCRIPT: &str = r#"
$ErrorActionPreference='Stop'
trap {[IO.File]::WriteAllText($env:NUMERA_FIXTURE_DIAGNOSTIC,$_.Exception.Message,[Text.UTF8Encoding]::new($false));exit 2}
[IO.File]::WriteAllText($env:NUMERA_FIXTURE_DIAGNOSTIC,'fixture-started',[Text.UTF8Encoding]::new($false))
$parts=$env:NUMERA_FIXTURE_ADDRESS.Split(':');$client=[Net.Sockets.TcpClient]::new()
$client.Connect($parts[0],[int]$parts[1]);$control=$client.GetStream();$control.ReadTimeout=5000;$control.WriteTimeout=2000
$pidBytes=[BitConverter]::GetBytes([uint32]$PID);$control.Write($pidBytes);$control.Flush()
if($env:NUMERA_FIXTURE_MODE -eq 'handshake'){$null=$control.ReadByte()}
$workerInput=[Console]::OpenStandardInput();$workerOutput=[Console]::OpenStandardOutput()
function Send-Frame([string]$Json){$bytes=[Text.Encoding]::UTF8.GetBytes($Json);$workerOutput.Write([BitConverter]::GetBytes([uint32]$bytes.Length));$workerOutput.Write($bytes);$workerOutput.Flush()}
Send-Frame '{"ready":true}'
[IO.File]::AppendAllText($env:NUMERA_FIXTURE_DIAGNOSTIC,'|ready-sent',[Text.UTF8Encoding]::new($false))
while($true){
    $length=[byte[]]::new(4);$first=$workerInput.ReadByte();if($first -lt 0){break};$length[0]=[byte]$first;$workerInput.ReadExactly($length,1,3)
    $count=[BitConverter]::ToUInt32($length);if($count -gt 8388608){exit 2};$content=[byte[]]::new($count);$workerInput.ReadExactly($content)
    [IO.File]::AppendAllText($env:NUMERA_FIXTURE_DIAGNOSTIC,'|request-read',[Text.UTF8Encoding]::new($false))
    $control.WriteByte(73);$control.Flush()
    if($env:NUMERA_FIXTURE_MODE -eq 'inference'){$null=$control.ReadByte()}
    if($env:NUMERA_FIXTURE_MODE -eq 'crash'){exit 0}
    Send-Frame '{"text":"owned","confidence":1.0,"device":"cpu","evidence":{}}'
}
$client.Dispose()
"#;

#[tokio::test]
async fn fixture_startup_and_binary_frame_roundtrip_without_engine() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let root = tempfile::tempdir().expect("root");
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("port");
    let executable = std::env::split_paths(&std::env::var_os("PATH").expect("PATH"))
        .map(|directory| directory.join("pwsh.exe"))
        .find(|path| path.is_file())
        .expect("PowerShell 7");
    let mut command = tokio::process::Command::new(executable);
    command
        .args([
            "-NoLogo",
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            FIXTURE_SCRIPT,
        ])
        .env(
            "NUMERA_FIXTURE_ADDRESS",
            listener.local_addr().expect("address").to_string(),
        )
        .env("NUMERA_FIXTURE_MODE", "ready")
        .env(
            "NUMERA_FIXTURE_DIAGNOSTIC",
            root.path().join("worker-diagnostic.txt"),
        )
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);
    #[cfg(windows)]
    command.creation_flags(0x08000000);
    let mut child = command.spawn().expect("owned fixture spawn");
    let mut diagnostic = child.stderr.take().expect("stderr");
    let stderr = tokio::spawn(async move {
        let mut bytes = Vec::new();
        let _ = diagnostic.read_to_end(&mut bytes).await;
        bytes
    });
    let operation = async {
        let (mut control, _address) = listener.accept().await.expect("connection");
        let _pid = control.read_u32_le().await.expect("pid");
        let output = child.stdout.as_mut().expect("stdout");
        let length = output.read_u32_le().await.expect("ready length");
        assert!(length < 1024);
        let mut frame = vec![0; length as usize];
        output.read_exact(&mut frame).await.expect("ready frame");
        assert_eq!(
            serde_json::from_slice::<Value>(&frame).expect("ready JSON")["ready"],
            true
        );
        let input = child.stdin.as_mut().expect("stdin");
        input.write_u32_le(5).await.expect("length");
        input.write_all(b"owned").await.expect("request");
        input.flush().await.expect("flush");
        let output = child.stdout.as_mut().expect("stdout");
        let length = output.read_u32_le().await.expect("response length");
        assert!(length < 1024);
        let mut frame = vec![0; length as usize];
        output.read_exact(&mut frame).await.expect("frame");
        assert_eq!(
            serde_json::from_slice::<Value>(&frame).expect("response")["text"],
            "owned"
        );
    };
    let result = tokio::time::timeout(Duration::from_secs(5), operation).await;
    let _ = child.start_kill();
    tokio::time::timeout(Duration::from_secs(3), child.wait())
        .await
        .expect("owned fixture reap")
        .expect("wait");
    let stderr = tokio::time::timeout(Duration::from_secs(1), stderr)
        .await
        .expect("stderr closes")
        .expect("diagnostic joined");
    let marker = std::fs::read_to_string(root.path().join("worker-diagnostic.txt"))
        .unwrap_or_else(|_| "fixture-not-started".into());
    assert!(
        result.is_ok(),
        "fixture timeout diagnostic={marker} stderr={}",
        String::from_utf8_lossy(&stderr)
    );
}

#[tokio::test]
async fn completed_response_without_caller_ack_cannot_publish_cached_worker() {
    use tokio::io::AsyncReadExt;
    let root = tempfile::tempdir().expect("root");
    let (engine, listener) = fixture_engine(root.path(), "ready", ready());
    let mut operation =
        Box::pin(engine.recognize(b"owned", Instant::now() + Duration::from_secs(5)));
    assert!(matches!(
        std::future::poll_fn(|context| std::task::Poll::Ready(operation.as_mut().poll(context)))
            .await,
        std::task::Poll::Pending
    ));
    let (mut control, pid) = entered(&listener).await;
    #[cfg(windows)]
    let process = ProcessHandle::open(pid);
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(2), control.read_u8())
            .await
            .expect("request entered")
            .expect("event"),
        b'I'
    );
    // Leave the caller future unpolled while the retained worker operation reaches its cache boundary.
    tokio::time::timeout(Duration::from_secs(2), async {
        while engine.worker_pid().await != Some(pid) {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("response cached pending caller acknowledgement");
    drop(operation);
    wait_idle(&engine).await;
    #[cfg(windows)]
    process.exited();
    assert_eq!(engine.worker_pid().await, None);
    let next = engine.clone();
    let successor = tokio::spawn(async move {
        next.recognize(b"successor", Instant::now() + Duration::from_secs(5))
            .await
    });
    let (_control, next_pid) = entered(&listener).await;
    #[cfg(windows)]
    let next_process = ProcessHandle::open(next_pid);
    assert_eq!(
        successor.await.expect("joined").expect("successor")["text"],
        "owned"
    );
    wait_idle(&engine).await;
    engine.close().await;
    #[cfg(windows)]
    next_process.exited();
}

#[tokio::test]
async fn handshake_and_inference_cancellation_reap_before_successor() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    for mode in ["handshake", "inference"] {
        let root = tempfile::tempdir().expect("isolated root");
        let (engine, listener) = fixture_engine(root.path(), mode, ready());
        let run = engine.clone();
        let mut task = tokio::spawn(async move {
            run.recognize(b"owned", Instant::now() + Duration::from_secs(5))
                .await
        });
        let event = tokio::select! {
            response=&mut task=>{engine.close().await;panic!("engine completed before fixture connection mode={mode} response={response:?}");},
            connection=tokio::time::timeout(Duration::from_secs(3), listener.accept())=>connection,
        };
        let (mut control, _address) = match event {
            Ok(Ok(value)) => value,
            _ => {
                let diagnostic = std::fs::read_to_string(root.path().join("worker-diagnostic.txt"))
                    .unwrap_or_else(|_| "fixture-not-started".into());
                task.abort();
                let _ = task.await;
                engine.close().await;
                panic!("worker connection diagnostic: {diagnostic}");
            }
        };
        let pid = control.read_u32_le().await.expect("pid");
        #[cfg(windows)]
        let process = ProcessHandle::open(pid);
        #[cfg(not(windows))]
        let _ = pid;
        if mode == "inference" {
            assert_eq!(
                tokio::time::timeout(Duration::from_secs(2), control.read_u8())
                    .await
                    .expect("request entered")
                    .expect("event"),
                b'I'
            );
        }
        task.abort();
        assert!(task.await.expect_err("caller cancellation").is_cancelled());
        wait_idle(&engine).await;
        #[cfg(windows)]
        process.exited();
        let next = engine.clone();
        let successor = tokio::spawn(async move {
            next.recognize(b"successor", Instant::now() + Duration::from_secs(5))
                .await
        });
        let (mut successor_control, next_pid) = entered(&listener).await;
        #[cfg(windows)]
        let next_process = ProcessHandle::open(next_pid);
        if mode == "handshake" {
            successor_control
                .write_all(b"R")
                .await
                .expect("release handshake");
        }
        if mode == "inference" {
            assert_eq!(
                tokio::time::timeout(Duration::from_secs(2), successor_control.read_u8())
                    .await
                    .expect("entered successor")
                    .expect("event"),
                b'I'
            );
            successor_control
                .write_all(b"R")
                .await
                .expect("release inference");
        }
        let result = successor.await.expect("joined successor");
        if result.is_err() {
            let diagnostic = std::fs::read_to_string(root.path().join("worker-diagnostic.txt"))
                .unwrap_or_else(|_| "fixture-no-diagnostic".into());
            engine.close().await;
            panic!(
                "successor failure {:?} diagnostic={diagnostic}",
                result.err()
            );
        }
        assert_eq!(result.expect("successor result")["text"], "owned");
        engine.close().await;
        #[cfg(windows)]
        next_process.exited();
        assert_eq!(
            engine
                .recognize(b"closed", Instant::now() + Duration::from_secs(1))
                .await
                .expect_err("closed engine")
                .code,
            "cancelled"
        );
    }
}

#[tokio::test]
async fn active_task_shutdown_joins_consumer_and_reaps_inference() {
    use numera_cha_solver::{
        core::contracts::Input, sdk::Numera, store::TaskStore, tasks::TaskService,
    };
    use tokio::io::AsyncReadExt;
    let root = tempfile::tempdir().expect("root");
    let (engine, listener) = fixture_engine(root.path(), "inference", ready());
    let mut encoded = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(2, 2, image::Rgb([255; 3])))
        .write_to(&mut encoded, image::ImageFormat::Png)
        .expect("fixture");
    let mut settings = Settings::new(root.path().into());
    settings.device = "cpu".into();
    let store = Arc::new(TaskStore::new(&root.path().join("tasks.sqlite3"), 60).expect("store"));
    let service = TaskService::new(Numera::with_engine(settings, engine.clone()), store.clone());
    let task = service
        .create(
            "active",
            Input {
                kind: "image".into(),
                asset: encoded.into_inner(),
                text: String::new(),
                subtype: "generic_text_line".into(),
                allowed_characters: String::new(),
                revision: String::new(),
            },
        )
        .await
        .expect("task");
    let id = task["id"].as_str().expect("task identity");
    let connection = tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let persisted = store.get(id).expect("owned task state");
            if persisted["state"] == "terminal" {
                return Err(format!("Task completed before worker startup: {persisted}"));
            }
            tokio::select! {
                connection = listener.accept() => return connection.map_err(|error| error.to_string()),
                _ = tokio::task::yield_now() => {},
            }
        }
    }).await;
    let (mut control, _) = match connection {
        Ok(Ok(connection)) => connection,
        failed => {
            let persisted = store.get(id).expect("failure task state");
            let diagnostic = std::fs::read_to_string(root.path().join("worker-diagnostic.txt"))
                .unwrap_or_else(|_| "fixture-not-started".into());
            service.close().await.expect("failed startup cleanup");
            panic!(
                "Active task startup failed: {failed:?}; task={persisted}; fixture={diagnostic}"
            );
        }
    };
    let pid = tokio::time::timeout(Duration::from_secs(1), control.read_u32_le())
        .await
        .expect("worker identity deadline")
        .expect("worker identity");
    #[cfg(windows)]
    let process = ProcessHandle::open(pid);
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(2), control.read_u8())
            .await
            .expect("request event")
            .expect("event"),
        b'I'
    );
    tokio::time::timeout(Duration::from_secs(5), service.close())
        .await
        .expect("joined service")
        .expect("durable task shutdown");
    assert_eq!(
        store
            .get(task["id"].as_str().expect("id"))
            .expect("terminal")["outcome"],
        "cancelled"
    );
    assert!(engine.idle().await);
    #[cfg(windows)]
    process.exited();
    assert!(
        !store
            .finish(
                task["id"].as_str().expect("id"),
                json!({"outcome":"recognized","answer":"late"})
            )
            .expect("late rejected")
    );
}

#[tokio::test]
async fn crashed_worker_is_reported_and_successor_owns_fresh_process() {
    use tokio::io::AsyncReadExt;
    let root = tempfile::tempdir().expect("root");
    let (engine, listener) = fixture_engine(root.path(), "crash", ready());
    for _ in 0..2 {
        let run = engine.clone();
        let task = tokio::spawn(async move {
            run.recognize(b"owned", Instant::now() + Duration::from_secs(5))
                .await
        });
        let (mut control, pid) = entered(&listener).await;
        #[cfg(windows)]
        let process = ProcessHandle::open(pid);
        let _ = tokio::time::timeout(Duration::from_secs(2), control.read_u8())
            .await
            .expect("request event");
        assert_eq!(
            task.await.expect("joined").expect_err("worker exit").code,
            "worker_failed"
        );
        wait_idle(&engine).await;
        #[cfg(windows)]
        process.exited();
    }
    engine.close().await;
}

#[tokio::test]
async fn owned_worker_deadline_is_expired_at_public_sdk_and_reaped() {
    use numera_cha_solver::{core::contracts::Input, sdk::Numera};
    let root = tempfile::tempdir().expect("isolated root");
    let (engine, listener) = fixture_engine(root.path(), "inference", ready());
    let mut encoded = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(2, 2, image::Rgb([255; 3])))
        .write_to(&mut encoded, image::ImageFormat::Png)
        .expect("image");
    let mut settings = Settings::new(root.path().into());
    settings.device = "cpu".into();
    let sdk = Numera::with_engine(settings, engine.clone());
    let input = Input {
        kind: "image".into(),
        asset: encoded.into_inner(),
        text: String::new(),
        subtype: "generic_text_line".into(),
        allowed_characters: String::new(),
        revision: String::new(),
    };
    let owner = sdk.clone();
    let task = tokio::spawn(async move {
        owner
            .recognize(input, Some(Instant::now() + Duration::from_secs(2)))
            .await
    });
    let (_control, pid) = entered(&listener).await;
    #[cfg(windows)]
    let process = ProcessHandle::open(pid);
    let result = tokio::time::timeout(Duration::from_secs(5), task)
        .await
        .expect("SDK operation ceiling")
        .expect("joined");
    assert_eq!(result.outcome, "expired");
    assert_eq!(result.subtype, "generic_text_line");
    assert_eq!(result.error.expect("expiry error").code, "expired");
    assert!(!result.server_verified);
    wait_idle(&engine).await;
    #[cfg(windows)]
    process.exited();
    sdk.close().await;
}

#[tokio::test]
async fn cancellation_during_verification_never_publishes_late_worker() {
    use std::sync::{Condvar, Mutex};
    let root = tempfile::tempdir().expect("root");
    let gate = Arc::new((Mutex::new(false), Condvar::new()));
    let barrier = gate.clone();
    let (started, mut enter) = tokio::sync::mpsc::channel(1);
    let verify = Arc::new(move || {
        let _ = started.try_send(());
        let (lock, event) = &*barrier;
        let (_guard, wait) = event
            .wait_timeout_while(lock.lock().expect("lock"), Duration::from_secs(3), |v| !*v)
            .expect("bounded gate");
        if wait.timed_out() {
            return Err(Error::new("expired", "Controlled verification timeout."));
        }
        Ok(json!({"availability":"ready"}))
    });
    let (engine, listener) = fixture_engine(root.path(), "ready", verify);
    let running = engine.clone();
    let task = tokio::spawn(async move {
        running
            .recognize(b"owned", Instant::now() + Duration::from_secs(5))
            .await
    });
    tokio::time::timeout(Duration::from_secs(1), enter.recv())
        .await
        .expect("entered")
        .expect("signal");
    task.abort();
    assert!(task.await.expect_err("cancelled caller").is_cancelled());
    assert_eq!(
        engine
            .recognize(b"successor", Instant::now() + Duration::from_secs(5))
            .await
            .expect_err("draining admission")
            .code,
        "inference_busy"
    );
    {
        let (lock, event) = &*gate;
        *lock.lock().expect("release") = true;
        event.notify_all();
    }
    wait_idle(&engine).await;
    let mut no_worker = Box::pin(listener.accept());
    assert!(matches!(
        std::future::poll_fn(|context| std::task::Poll::Ready(no_worker.as_mut().poll(context)))
            .await,
        std::task::Poll::Pending
    ));
    let next = engine.clone();
    let successor = tokio::spawn(async move {
        next.recognize(b"successor", Instant::now() + Duration::from_secs(5))
            .await
    });
    let (_control, pid) = entered(&listener).await;
    #[cfg(windows)]
    let process = ProcessHandle::open(pid);
    assert_eq!(
        successor.await.expect("joined").expect("successor")["text"],
        "owned"
    );
    engine.close().await;
    #[cfg(windows)]
    process.exited();
}
