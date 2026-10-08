use numera_cha_solver::{config::Settings, ownership::Ownership, startup};
use std::net::TcpListener;

#[test]
fn credential_publication_replaces_atomically_and_failure_preserves_previous() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("api-token.txt");
    startup::publish_credential(&path, "owned-old-fixture").unwrap();
    startup::publish_credential(&path, "owned-new-fixture").unwrap();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "owned-new-fixture");
    let directory = root.path().join("existing-directory");
    std::fs::create_dir(&directory).unwrap();
    assert!(startup::publish_credential(&directory, "synthetic-value").is_err());
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 2);
}

#[tokio::test]
async fn competing_start_occupied_port_and_failed_store_preserve_credential_and_release_owner() {
    let root = tempfile::tempdir().unwrap();
    let settings = Settings::new(root.path().into());
    std::fs::create_dir_all(settings.data_dir()).unwrap();
    let credential = settings.data_dir().join("api-token.txt");
    std::fs::write(&credential, "owned-old-fixture").unwrap();
    let path = settings.data_dir().join(".server-owner.lock");
    let lease = Ownership::acquire(&path).unwrap();
    assert_eq!(
        startup::serve(settings.clone(), "127.0.0.1", 8765)
            .await
            .unwrap_err()
            .code,
        "server_busy"
    );
    drop(lease);
    let occupied = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = occupied.local_addr().unwrap().port();
    assert_eq!(
        startup::serve(settings.clone(), "127.0.0.1", port)
            .await
            .unwrap_err()
            .code,
        "startup_failed"
    );
    assert_eq!(
        std::fs::read_to_string(&credential).unwrap(),
        "owned-old-fixture"
    );
    drop(Ownership::acquire(&path).unwrap());
    drop(occupied);
    std::fs::create_dir(settings.data_dir().join("tasks.sqlite3")).unwrap();
    let reserve = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = reserve.local_addr().unwrap().port();
    drop(reserve);
    assert_eq!(
        startup::serve(settings, "127.0.0.1", port)
            .await
            .unwrap_err()
            .code,
        "store_failed"
    );
    assert_eq!(
        std::fs::read_to_string(credential).unwrap(),
        "owned-old-fixture"
    );
    drop(Ownership::acquire(&path).unwrap());
    let rebound = TcpListener::bind(("127.0.0.1", port)).unwrap();
    drop(rebound);
}

#[tokio::test]
async fn bound_server_socket_is_exclusive_on_windows() {
    let listener = startup::bind_loopback("127.0.0.1", 0).unwrap();
    #[cfg(windows)]
    {
        use std::os::windows::io::AsRawSocket;
        use windows_sys::Win32::Networking::WinSock::{
            SO_EXCLUSIVEADDRUSE, SOL_SOCKET, getsockopt,
        };
        let mut enabled = 0i32;
        let mut length = std::mem::size_of::<i32>() as i32;
        // SAFETY: listener owns a live socket; output buffers are valid and correctly sized.
        let status = unsafe {
            getsockopt(
                listener.as_raw_socket() as usize,
                SOL_SOCKET,
                SO_EXCLUSIVEADDRUSE,
                (&mut enabled as *mut i32).cast(),
                &mut length,
            )
        };
        assert_eq!(status, 0);
        assert_eq!(enabled, 1);
    }
    assert!(TcpListener::bind(listener.local_addr().unwrap()).is_err());
}
