use crate::{api, config::Settings, core::contracts::Error, ownership::Ownership};
use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::Path,
    process::{Command, Stdio},
};

pub fn publish_credential(path: &Path, token: &str) -> Result<(), Error> {
    let parent = path
        .parent()
        .ok_or_else(|| Error::new("credential_permissions", "Credential path is unavailable."))?;
    fs::create_dir_all(parent).map_err(|_| {
        Error::new(
            "credential_permissions",
            "Credential directory could not be created.",
        )
    })?;
    let temporary = parent.join(format!(
        "credential-{}-{:x}",
        std::process::id(),
        rand::random::<u64>()
    ));
    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .map_err(|_| {
                Error::new(
                    "credential_permissions",
                    "Local credential could not be created.",
                )
            })?;
        file.write_all(token.as_bytes())
            .and_then(|_| file.sync_all())
            .map_err(|_| {
                Error::new(
                    "credential_permissions",
                    "Local credential could not be written.",
                )
            })?;
        drop(file);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            let mut whoami = Command::new("whoami");
            whoami
                .args(["/user", "/fo", "csv", "/nh"])
                .creation_flags(0x08000000);
            let output = bounded(&mut whoami)?;
            let line = String::from_utf8(output).map_err(|_| {
                Error::new(
                    "credential_permissions",
                    "Owner identity could not be resolved.",
                )
            })?;
            let sid = line
                .trim()
                .split(',')
                .nth(1)
                .map(|s| s.trim_matches('"'))
                .filter(|s| s.starts_with("S-1-"))
                .ok_or_else(|| {
                    Error::new(
                        "credential_permissions",
                        "Owner identity could not be resolved.",
                    )
                })?;
            let mut icacls = Command::new("icacls");
            icacls
                .arg(&temporary)
                .args(["/inheritance:r", "/grant:r", &format!("*{sid}:F")])
                .creation_flags(0x08000000);
            bounded(&mut icacls)?;
        }
        fs::rename(&temporary, path).map_err(|_| {
            Error::new(
                "credential_permissions",
                "Local credential could not be atomically published.",
            )
        })?;
        Ok(())
    })();
    if temporary.exists() {
        let _ = fs::remove_file(&temporary);
    }
    result
}
#[cfg(windows)]
fn bounded(command: &mut Command) -> Result<Vec<u8>, Error> {
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| {
            Error::new(
                "credential_permissions",
                "Credential permission helper could not start.",
            )
        })?;
    let start = std::time::Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                if !status.success() {
                    return Err(Error::new(
                        "credential_permissions",
                        "Local credential ACL could not be restricted.",
                    ));
                }
                return child.wait_with_output().map(|o| o.stdout).map_err(|_| {
                    Error::new(
                        "credential_permissions",
                        "Credential permission output unavailable.",
                    )
                });
            }
            Ok(None) if start.elapsed() < std::time::Duration::from_secs(5) => {
                std::thread::sleep(std::time::Duration::from_millis(10))
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(Error::new(
                    "credential_permissions",
                    "Credential permission helper exceeded its deadline.",
                ));
            }
        }
    }
}
pub fn bind_loopback(host: &str, port: u16) -> std::io::Result<tokio::net::TcpListener> {
    if !matches!(host, "127.0.0.1" | "localhost") {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "Loopback host is required.",
        ));
    }
    let socket = tokio::net::TcpSocket::new_v4()?;
    #[cfg(windows)]
    {
        use std::os::windows::io::AsRawSocket;
        use windows_sys::Win32::Networking::WinSock::{
            SO_EXCLUSIVEADDRUSE, SOL_SOCKET, WSAGetLastError, setsockopt,
        };
        let enabled = 1i32;
        // SAFETY: socket owns a live unbound handle and the option buffer lives through the call.
        if unsafe {
            setsockopt(
                socket.as_raw_socket() as usize,
                SOL_SOCKET,
                SO_EXCLUSIVEADDRUSE,
                (&enabled as *const i32).cast(),
                std::mem::size_of::<i32>() as i32,
            )
        } != 0
        {
            // SAFETY: reads the calling thread's last Winsock error, immediately after failure.
            return Err(std::io::Error::from_raw_os_error(unsafe {
                WSAGetLastError()
            }));
        }
    }
    socket.bind(std::net::SocketAddr::from(([127, 0, 0, 1], port)))?;
    socket.listen(128)
}
pub async fn serve(mut settings: Settings, host: &str, port: u16) -> Result<(), Error> {
    let _owner = Ownership::acquire(&settings.data_dir().join(".server-owner.lock"))
        .map_err(|_| Error::new("server_busy", "Another application owns this local root."))?;
    let listener = bind_loopback(host, port)
        .map_err(|_| Error::new("startup_failed", "Local server port could not be bound."))?;
    let generated = settings.api_token.is_empty();
    if generated {
        settings.api_token = api::token();
    }
    let state = api::state(settings.clone())?;
    if generated {
        if let Err(error) = publish_credential(
            &settings.data_dir().join("api-token.txt"),
            &settings.api_token,
        ) {
            if let Err(cleanup) = state.tasks.close().await {
                crate::logging::event(
                    "ERROR",
                    "cleanup",
                    &format!("Startup task cleanup failed code={}", cleanup.code),
                );
            }
            state.readiness.close().await;
            return Err(error);
        }
        crate::logging::event(
            "INFO",
            "config",
            "Local API credential published; value not printed.",
        );
    }
    crate::logging::event(
        "INFO",
        "startup",
        &format!("Authenticated loopback server port={port}"),
    );
    let result = axum::serve(listener, api::router(state.clone()))
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await;
    let cleanup = state.tasks.close().await;
    state.readiness.close().await;
    cleanup?;
    result.map_err(|_| Error::new("startup_failed", "Local server execution failed."))
}
