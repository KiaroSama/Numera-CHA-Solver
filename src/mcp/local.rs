use super::tools::Tools;
use crate::{config::Settings, core::contracts::Error, logging, review::ReviewStore, sdk::Numera};
use rmcp::{
    RoleServer, ServiceExt,
    service::QuitReason,
    transport::{IntoTransport, stdio},
};
use std::time::Duration;
use tokio::time::timeout;

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

async fn run_transport<T, E, A>(tools: Tools, transport: T) -> Result<(), Error>
where
    T: IntoTransport<RoleServer, E, A>,
    E: std::error::Error + Send + Sync + 'static,
{
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
    sdk.close().await;
    if !sdk.decode_idle() {
        logging::event(
            "ERROR",
            "mcp-stdio",
            "Owned decoder did not finish draining.",
        );
        return Err(failure());
    }
    logging::event(
        "INFO",
        "mcp-stdio",
        "Client-owned local MCP resources closed.",
    );
    result
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
