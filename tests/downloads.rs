use numera_cha_solver::{config::Settings, core::contracts::Error, models::ModelRegistry};
use std::{
    io::{Read, Write},
    net::TcpListener,
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

struct Owned(Child);
impl Drop for Owned {
    fn drop(&mut self) {
        if self.0.try_wait().ok().flatten().is_none() {
            let _ = self.0.kill();
        }
        let _ = self.0.wait();
    }
}

#[test]
fn download_fixture_child() {
    let Ok(root) = std::env::var("NUMERA_DOWNLOAD_FIXTURE_ROOT") else {
        return;
    };
    let signal = std::env::var("NUMERA_DOWNLOAD_FIXTURE_SIGNAL").unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let registry = ModelRegistry::new(Settings::new(root.into()));
        let result = registry
            .fetch_with("ppocr-v6-small", move |_, _, _| {
                let signal = signal.clone();
                async move {
                    let mut control = std::net::TcpStream::connect(signal).unwrap();
                    control
                        .set_read_timeout(Some(Duration::from_secs(10)))
                        .unwrap();
                    control.write_all(b"entered").unwrap();
                    let mut response = [0];
                    let _ = control.read_exact(&mut response);
                    Err(Error::new(
                        "offline_boundary",
                        "Controlled offline download boundary.",
                    ))
                }
            })
            .await;
        assert!(result.is_err());
    });
}

#[tokio::test]
async fn stale_download_marker_and_crashed_live_download_owner_recover_without_network() {
    let root = tempfile::tempdir().unwrap();
    let registry = ModelRegistry::new(Settings::new(root.path().into()));
    let directory = registry.directory("ppocr-v6-small").unwrap();
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(directory.join(".download.lock"), b"stale").unwrap();
    let offline = |_, _, _| async {
        Err(Error::new(
            "offline_boundary",
            "Controlled offline download boundary.",
        ))
    };
    assert_eq!(
        registry
            .fetch_with("ppocr-v6-small", offline)
            .await
            .unwrap_err()
            .code,
        "offline_boundary"
    );
    let control = TcpListener::bind("127.0.0.1:0").unwrap();
    control.set_nonblocking(true).unwrap();
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args(["--exact", "download_fixture_child", "--nocapture"])
        .env("NUMERA_DOWNLOAD_FIXTURE_ROOT", root.path())
        .env(
            "NUMERA_DOWNLOAD_FIXTURE_SIGNAL",
            control.local_addr().unwrap().to_string(),
        )
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let mut child = Owned(command.spawn().unwrap());
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut stream = loop {
        match control.accept() {
            Ok((stream, _)) => break stream,
            Err(error)
                if error.kind() == std::io::ErrorKind::WouldBlock && Instant::now() < deadline =>
            {
                std::thread::yield_now()
            }
            Err(error) => panic!("Owned download control failed: {error}"),
        }
    };
    stream
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    let mut entered = [0; 7];
    stream.read_exact(&mut entered).unwrap();
    assert_eq!(&entered, b"entered");
    assert_eq!(
        registry
            .fetch_with("ppocr-v6-small", offline)
            .await
            .unwrap_err()
            .code,
        "model_busy"
    );
    child.0.kill().unwrap();
    child.0.wait().unwrap();
    assert!(child.0.try_wait().unwrap().is_some());
    assert_eq!(
        registry
            .fetch_with("ppocr-v6-small", offline)
            .await
            .unwrap_err()
            .code,
        "offline_boundary"
    );
}
