use super::tools::Tools;
use crate::{config::Settings, core::contracts::Error, review::ReviewStore, sdk::Numera};
use axum::{
    Router,
    body::{Body, to_bytes},
    extract::{Request, State},
    http::{StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Response},
};
use rmcp::transport::streamable_http_server::{
    StreamableHttpServerConfig, StreamableHttpService, session::local::LocalSessionManager,
};
use std::{sync::Arc, time::Duration};
use tokio_util::sync::CancellationToken;

#[derive(Clone)]
struct Boundary {
    token: Arc<String>,
    port: u16,
}
fn constant_eq(a: &[u8], b: &[u8]) -> bool {
    let mut diff = a.len() ^ b.len();
    for index in 0..a.len().max(b.len()) {
        diff |= (a.get(index).copied().unwrap_or(0) ^ b.get(index).copied().unwrap_or(0)) as usize;
    }
    diff == 0
}
async fn boundary(State(state): State<Boundary>, request: Request, next: Next) -> Response {
    let host = request
        .headers()
        .get(header::HOST)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    if host != format!("127.0.0.1:{}", state.port) && host != format!("localhost:{}", state.port) {
        return StatusCode::FORBIDDEN.into_response();
    }
    if let Some(origin) = request.headers().get(header::ORIGIN) {
        if origin.to_str().ok() != Some(format!("http://{host}").as_str()) {
            return StatusCode::FORBIDDEN.into_response();
        }
    }
    let token = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .unwrap_or("");
    if token.is_empty() || !constant_eq(token.as_bytes(), state.token.as_bytes()) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let (parts, body) = request.into_parts();
    let bytes =
        match tokio::time::timeout(Duration::from_secs(10), to_bytes(body, 11_190_000)).await {
            Ok(Ok(bytes)) => bytes,
            Ok(Err(_)) => return StatusCode::PAYLOAD_TOO_LARGE.into_response(),
            Err(_) => return StatusCode::REQUEST_TIMEOUT.into_response(),
        };
    next.run(Request::from_parts(parts, Body::from(bytes)))
        .await
}
pub fn router(tools: Tools, token: String, port: u16, cancel: CancellationToken) -> Router {
    let service = StreamableHttpService::new(
        move || Ok(tools.clone()),
        LocalSessionManager::default().into(),
        StreamableHttpServerConfig::default()
            .with_max_request_body_bytes(11_190_000)
            .with_cancellation_token(cancel),
    );
    Router::new()
        .nest_service("/mcp", service)
        .layer(middleware::from_fn_with_state(
            Boundary {
                token: Arc::new(token),
                port,
            },
            boundary,
        ))
}
pub async fn serve(settings: Settings, port: u16) -> Result<(), Error> {
    let _owner =
        crate::ownership::Ownership::acquire(&settings.data_dir().join(".mcp-owner.lock"))?;
    let listener = crate::startup::bind_loopback("127.0.0.1", port)
        .map_err(|_| Error::new("mcp_startup_failed", "MCP loopback port is unavailable."))?;
    let token = match std::env::var("NUMERA_MCP_TOKEN") {
        Ok(value)
            if value.len() >= 16 && value.len() <= 4096 && !value.chars().any(char::is_control) =>
        {
            value
        }
        Ok(_) => {
            return Err(Error::new(
                "invalid_config",
                "MCP token must be a bounded secret of at least16bytes.",
            ));
        }
        Err(std::env::VarError::NotPresent) => {
            let path = settings.data_dir().join("mcp-token.txt");
            match crate::assets::open_regular(&path) {
                Ok(file) => {
                    use std::io::Read;
                    let mut value = String::new();
                    file.take(4097).read_to_string(&mut value).map_err(|_| {
                        Error::new("invalid_config", "MCP credential is unreadable.")
                    })?;
                    if value.len() < 16 || value.len() > 4096 || value.chars().any(char::is_control)
                    {
                        return Err(Error::new("invalid_config", "MCP credential is invalid."));
                    }
                    value
                }
                Err(_) if !path.exists() => {
                    let value = crate::api::token();
                    crate::startup::publish_credential(&path, &value)?;
                    value
                }
                Err(_) => {
                    return Err(Error::new(
                        "invalid_config",
                        "MCP credential is not a regular local file.",
                    ));
                }
            }
        }
        Err(_) => {
            return Err(Error::new(
                "invalid_config",
                "MCP token contains invalidUnicode.",
            ));
        }
    };
    let tools = Tools {
        sdk: Numera::new(settings.clone()),
        reviews: ReviewStore::new(
            &settings.data_dir().join("reviews.sqlite3"),
            settings.result_ttl,
        )?,
    };
    let sdk = tools.sdk.clone();
    let cancel = CancellationToken::new();
    let stop = cancel.clone();
    crate::logging::event(
        "INFO",
        "mcp",
        &format!("Authenticated loopback MCP started port={port}; no credential printed."),
    );
    let result = axum::serve(listener, router(tools, token, port, cancel))
        .with_graceful_shutdown(async move {
            let _ = tokio::signal::ctrl_c().await;
            stop.cancel();
        })
        .await;
    sdk.close().await;
    result.map_err(|_| {
        Error::new(
            "mcp_startup_failed",
            "MCP server stopped with a transport error.",
        )
    })
}
