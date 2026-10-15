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
    // BSD accepts can inherit O_NONBLOCK; the bounded handshake requires blocking I/O.
    stream.set_nonblocking(false).unwrap();
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

#[tokio::test]
async fn cancelled_and_expired_downloads_release_the_exact_lease() {
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };
    struct Dropped(Arc<AtomicBool>);
    impl Drop for Dropped {
        fn drop(&mut self) {
            self.0.store(true, Ordering::SeqCst);
        }
    }
    for cancel in [true, false] {
        let root = tempfile::tempdir().unwrap();
        let mut settings = Settings::new(root.path().into());
        settings.deadline_seconds = 1.0;
        let registry = ModelRegistry::new(settings);
        let lease = registry
            .directory("ppocr-v6-small")
            .unwrap()
            .join(".download.lock");
        let dropped = Arc::new(AtomicBool::new(false));
        let flag = dropped.clone();
        let (entered, ready) = tokio::sync::oneshot::channel();
        let entered = std::sync::Mutex::new(Some(entered));
        let mut task = tokio::spawn(async move {
            registry
                .fetch_with("ppocr-v6-small", move |_, _, _| {
                    let guard = Dropped(flag.clone());
                    let entered = entered.lock().unwrap().take().unwrap();
                    async move {
                        let _guard = guard;
                        entered.send(()).unwrap();
                        std::future::pending::<Result<(), Error>>().await
                    }
                })
                .await
        });
        if !matches!(
            tokio::time::timeout(Duration::from_secs(3), ready).await,
            Ok(Ok(()))
        ) {
            task.abort();
            let _ = task.await;
            panic!("Owned download did not enter its controlled transport.");
        }
        if cancel {
            task.abort();
        }
        let result = match tokio::time::timeout(Duration::from_secs(3), &mut task).await {
            Ok(result) => result,
            Err(_) => {
                task.abort();
                let _ = task.await;
                panic!("Owned download did not stop within its budget.");
            }
        };
        if cancel {
            assert!(result.unwrap_err().is_cancelled());
        } else {
            assert_eq!(result.unwrap().unwrap_err().code, "expired");
        }
        assert!(dropped.load(Ordering::SeqCst));
        let mut owner = numera_cha_solver::ownership::Ownership::acquire(&lease).unwrap();
        owner.close();
        let registry = ModelRegistry::new(Settings::new(root.path().into()));
        assert_eq!(
            registry
                .fetch_with("ppocr-v6-small", |_, _, _| async {
                    Err(Error::new(
                        "offline_boundary",
                        "Owned successor reached transport.",
                    ))
                })
                .await
                .unwrap_err()
                .code,
            "offline_boundary"
        );
    }
}

#[tokio::test]
async fn successful_transport_cannot_bypass_missing_or_corrupt_asset_verification() {
    for corrupt in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let registry = ModelRegistry::new(Settings::new(root.path().into()));
        let result = registry
            .fetch_with("ppocr-v6-small", move |_, path, _| async move {
                if corrupt {
                    tokio::fs::write(path, b"owned corrupt fixture")
                        .await
                        .unwrap();
                }
                Ok(())
            })
            .await
            .unwrap();
        assert_eq!(
            result["availability"],
            if corrupt {
                "model_corrupt"
            } else {
                "model_missing"
            }
        );
        assert!(!result["failures"].as_array().unwrap().is_empty());
        let lease = registry
            .directory("ppocr-v6-small")
            .unwrap()
            .join(".download.lock");
        assert!(numera_cha_solver::ownership::Ownership::acquire(&lease).is_ok());
    }
}

#[tokio::test]
async fn directory_lock_refuses_transport_without_deleting_user_content() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let root = tempfile::tempdir().unwrap();
    let registry = ModelRegistry::new(Settings::new(root.path().into()));
    let lock = registry
        .directory("ppocr-v6-small")
        .unwrap()
        .join(".download.lock");
    std::fs::create_dir_all(&lock).unwrap();
    let sentinel = lock.join("retained.txt");
    std::fs::write(&sentinel, b"owned sentinel").unwrap();
    let calls = AtomicUsize::new(0);
    let result = registry
        .fetch_with("ppocr-v6-small", |_, _, _| {
            calls.fetch_add(1, Ordering::SeqCst);
            async { Ok(()) }
        })
        .await;
    assert_eq!(result.unwrap_err().code, "download_failed");
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert_eq!(std::fs::read(sentinel).unwrap(), b"owned sentinel");
}

#[cfg(unix)]
#[tokio::test]
async fn symlink_lock_cannot_modify_its_external_target() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let root = tempfile::tempdir().unwrap();
    let target = tempfile::tempdir().unwrap();
    let sentinel = target.path().join("retained.txt");
    std::fs::write(&sentinel, b"owned external sentinel").unwrap();
    let registry = ModelRegistry::new(Settings::new(root.path().into()));
    let directory = registry.directory("ppocr-v6-small").unwrap();
    std::fs::create_dir_all(&directory).unwrap();
    std::os::unix::fs::symlink(&sentinel, directory.join(".download.lock")).unwrap();
    let calls = AtomicUsize::new(0);
    let result = registry
        .fetch_with("ppocr-v6-small", |_, _, _| {
            calls.fetch_add(1, Ordering::SeqCst);
            async { Ok(()) }
        })
        .await;
    assert_eq!(result.unwrap_err().code, "download_failed");
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert_eq!(std::fs::read(sentinel).unwrap(), b"owned external sentinel");
}
