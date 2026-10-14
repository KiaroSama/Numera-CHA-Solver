use numera_cha_solver::{api, config::Settings, logging, startup};
use serde::Serialize;
use std::{sync::Mutex, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    runtime::Runtime,
    task::JoinHandle,
};
use tokio_util::sync::CancellationToken;

#[derive(Clone, Serialize)]
pub struct ApiConnection {
    pub url: String,
    pub token: String,
}

pub struct Backend {
    pub connection: ApiConnection,
    runtime: Runtime,
    cancel: CancellationToken,
    server: Mutex<Option<JoinHandle<Result<(), numera_cha_solver::core::contracts::Error>>>>,
    _log: logging::LogGuard,
}

impl Backend {
    pub fn start(mut settings: Settings) -> Result<Self, Box<dyn std::error::Error>> {
        settings.validate()?;
        let log = logging::initialize(&settings.log_dir(), "numera-cha-desktop", false);
        logging::event("INFO", "desktop", "Starting owned embedded loopback API.");
        if settings.device == "cpu" {
            logging::event(
                "WARNING",
                "device",
                "CPU selected; bundled desktop runtime has no verified accelerator provider.",
            );
        }
        settings.api_token = api::token();
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()?;
        let listener = {
            let _entered = runtime.enter();
            startup::bind_loopback("127.0.0.1", 0)?
        };
        let address = listener.local_addr()?;
        let connection = ApiConnection {
            url: format!("http://127.0.0.1:{}", address.port()),
            token: settings.api_token.clone(),
        };
        let cancel = CancellationToken::new();
        let server = runtime.spawn(startup::serve_listener_with_shutdown(
            settings,
            listener,
            cancel.clone(),
        ));
        let backend = Self {
            connection,
            runtime,
            cancel,
            server: Mutex::new(Some(server)),
            _log: log,
        };
        if let Err(error) = backend.runtime.block_on(ready(address)) {
            backend.stop();
            return Err(error.into());
        }
        logging::event(
            "INFO",
            "desktop",
            "Owned loopback API is ready; credential not logged.",
        );
        Ok(backend)
    }

    pub fn stop(&self) -> bool {
        self.cancel.cancel();
        let Ok(mut server) = self.server.lock() else {
            logging::event(
                "ERROR",
                "cleanup",
                "Desktop server ownership state is unavailable.",
            );
            return false;
        };
        let Some(mut handle) = server.take() else {
            return true;
        };
        let clean = self.runtime.block_on(async {
            match tokio::time::timeout(Duration::from_secs(15), &mut handle).await {
                Ok(Ok(Ok(()))) => true,
                Ok(_) => false,
                Err(_) => {
                    handle.abort();
                    let _ = handle.await;
                    false
                }
            }
        });
        logging::event(
            if clean { "INFO" } else { "ERROR" },
            "cleanup",
            if clean {
                "Desktop API and owned tasks drained."
            } else {
                "Desktop shutdown failed its bounded drain."
            },
        );
        clean
    }
}

impl Drop for Backend {
    fn drop(&mut self) {
        self.stop();
    }
}

async fn ready(address: std::net::SocketAddr) -> Result<(), std::io::Error> {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let mut stream = tokio::net::TcpStream::connect(address).await?;
            stream
                .write_all(
                    format!(
                        "GET /health/live HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\n\r\n"
                    )
                    .as_bytes(),
                )
                .await?;
            let mut response = [0; 256];
            let size = stream.read(&mut response).await?;
            if response[..size].starts_with(b"HTTP/1.1 200") {
                return Ok(());
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .map_err(|_| {
        std::io::Error::new(
            std::io::ErrorKind::TimedOut,
            "Desktop API readiness deadline exhausted.",
        )
    })?
}
