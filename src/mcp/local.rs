use super::tools::Tools;
use crate::{config::Settings, core::contracts::Error, logging, review::ReviewStore, sdk::Numera};
use rmcp::{ServiceExt, service::QuitReason, transport::stdio};
use std::{
    pin::Pin,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    task::{Context, Poll},
    time::Duration,
};
use tokio::{
    io::{AsyncRead, AsyncWrite, ReadBuf},
    time::timeout,
};

const FRAME_LIMIT: usize = 11_190_000;

// Bound each JSON-RPC line before rmcp allocates/deserializes the full message.
struct BoundedInput<R> {
    inner: R,
    length: usize,
    rejected: Arc<AtomicBool>,
}
impl<R: AsyncRead + Unpin> AsyncRead for BoundedInput<R> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        let before = buf.filled().len();
        match Pin::new(&mut self.inner).poll_read(cx, buf) {
            Poll::Ready(Ok(())) => {
                for &byte in &buf.filled()[before..] {
                    if byte == b'\n' {
                        self.length = 0;
                    } else {
                        self.length += 1;
                        if self.length > FRAME_LIMIT {
                            self.rejected.store(true, Ordering::Release);
                            return Poll::Ready(Err(std::io::Error::new(
                                std::io::ErrorKind::InvalidData,
                                "MCP frame exceeds its byte limit.",
                            )));
                        }
                    }
                }
                Poll::Ready(Ok(()))
            }
            other => other,
        }
    }
}

#[cfg(test)]
#[path = "local_tests.rs"]
mod tests;

/// The requesting client owns this runtime; no separate HTTP service or credential is needed.
pub async fn run(settings: Settings) -> Result<(), Error> {
    let reviews = ReviewStore::new(
        &settings.data_dir().join("reviews.sqlite3"),
        settings.result_ttl,
    )?;
    let tools = Tools {
        sdk: Numera::new(settings),
        reviews,
    };
    run_transport(tools, stdio()).await
}

async fn run_transport<R, W>(tools: Tools, (read, write): (R, W)) -> Result<(), Error>
where
    R: AsyncRead + Send + Unpin + 'static,
    W: AsyncWrite + Send + Unpin + 'static,
{
    let rejected = Arc::new(AtomicBool::new(false));
    let transport = (
        BoundedInput {
            inner: read,
            length: 0,
            rejected: rejected.clone(),
        },
        write,
    );
    let sdk = tools.sdk.clone();
    logging::event(
        "INFO",
        "mcp-stdio",
        "Starting client-owned local MCP; no HTTP server required.",
    );
    let result = match timeout(Duration::from_secs(15), tools.serve(transport)).await {
        Ok(Ok(service)) => {
            let cancel = service.cancellation_token();
            let waiting = service.waiting();
            tokio::pin!(waiting);
            tokio::select! {
                result = &mut waiting => orderly(result),
                _ = tokio::signal::ctrl_c() => {
                    cancel.cancel();
                    match timeout(Duration::from_secs(5), &mut waiting).await {
                        Ok(result) => orderly(result),
                        Err(_) => Err(failure()),
                    }
                }
            }
        }
        _ => Err(failure()),
    };
    // Cancellation stops admission before the actual decoder/worker is drained.
    if !sdk.close().await {
        logging::event(
            "ERROR",
            "mcp-stdio",
            "Owned decoder or inference worker did not finish draining.",
        );
        return Err(failure());
    }
    logging::event(
        "INFO",
        "mcp-stdio",
        "Client-owned local MCP resources closed.",
    );
    if rejected.load(Ordering::Acquire) {
        Err(failure())
    } else {
        result
    }
}
fn orderly(result: Result<QuitReason, tokio::task::JoinError>) -> Result<(), Error> {
    match result {
        Ok(QuitReason::Closed | QuitReason::Cancelled) => Ok(()),
        _ => Err(failure()),
    }
}
fn failure() -> Error {
    Error::new(
        "mcp_stdio_failed",
        "Local MCP client initialization or shutdown failed; inspect the local diagnostic log.",
    )
}
