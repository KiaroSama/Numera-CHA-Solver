use crate::{config::Settings, core::contracts::Error};
use rmcp::{
    ErrorData, RoleClient, RoleServer, ServerHandler, ServiceExt,
    model::{
        CallToolRequestParams, CallToolResponse, ClientCapabilities, ClientConfig, Implementation,
        ListToolsResult, PaginatedRequestParams, ServerCapabilities, ServerInfo,
    },
    service::{Peer, RequestContext},
    transport::{
        StreamableHttpClientTransport, stdio,
        streamable_http_client::StreamableHttpClientTransportConfig,
    },
};
use std::{io::Read, time::Duration};

#[derive(Clone)]
struct Bridge {
    peer: Peer<RoleClient>,
}
impl ServerHandler for Bridge {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("Numera-CHA-Solver-MCP",env!("CARGO_PKG_VERSION")))
            .with_instructions("Bridge to the selected authenticated local instance. Never impersonate human confirmation or bypass site protections.")
    }
    async fn list_tools(
        &self,
        request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        tokio::time::timeout(Duration::from_secs(10), self.peer.list_tools(request))
            .await
            .map_err(|_| ErrorData::internal_error("Local tool discovery timed out.", None))?
            .map_err(service_error)
    }
    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        if request.name == "confirm_review" {
            return Err(ErrorData::invalid_params(
                "Human confirmation is not an agent tool.",
                None,
            ));
        }
        let message =
            rmcp::model::ClientRequest::CallToolRequest(rmcp::model::CallToolRequest::new(request));
        let mut handle = self
            .peer
            .send_cancellable_request(
                message,
                rmcp::service::PeerRequestOptions::with_timeout(Duration::from_secs(125)),
            )
            .await
            .map_err(|_| ErrorData::internal_error("Local call could not start.", None))?;
        let received = tokio::select! {
            value=tokio::time::timeout(Duration::from_secs(125),&mut handle.rx)=>Some(value),
            _=context.ct.cancelled()=>None,
        };
        let value = match received {
            Some(Ok(Ok(Ok(value)))) => value,
            Some(Ok(Ok(Err(error)))) => return Err(service_error(error)),
            Some(Ok(Err(_))) => {
                return Err(ErrorData::internal_error("Local response closed.", None));
            }
            failure => {
                let _ = tokio::time::timeout(
                    Duration::from_secs(5),
                    handle.cancel(Some("Bridge caller stopped or timed out.".into())),
                )
                .await;
                return Err(ErrorData::internal_error(
                    if failure.is_none() {
                        "Bridge call cancelled."
                    } else {
                        "Local tool failed or exhausted its deadline."
                    },
                    None,
                ));
            }
        };
        match value {
            rmcp::model::ServerResult::CallToolResult(result) => {
                Ok(CallToolResponse::Complete(result))
            }
            _ => Err(ErrorData::internal_error(
                "Unexpected local tool response.",
                None,
            )),
        }
    }
}
fn service_error(error: rmcp::service::ServiceError) -> ErrorData {
    match error {
        rmcp::service::ServiceError::McpError(error) => error,
        _ => ErrorData::internal_error("Local transport failed.", None),
    }
}
fn unavailable() -> Error {
    Error::new(
        "mcp_unavailable",
        "Start the configured local MCP server first; bridge will not use an alternate endpoint.",
    )
}
pub async fn run(settings: Settings, port: u16) -> Result<(), Error> {
    let token = if let Ok(value) = std::env::var("NUMERA_MCP_TOKEN") {
        value
    } else {
        let file = crate::assets::open_regular(&settings.data_dir().join("mcp-token.txt"))
            .map_err(|_| unavailable())?;
        let mut value = String::new();
        file.take(4097)
            .read_to_string(&mut value)
            .map_err(|_| unavailable())?;
        value
    };
    if token.len() < 16 || token.len() > 4096 || token.chars().any(char::is_control) {
        return Err(unavailable());
    }
    let client = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(130))
        .build()
        .map_err(|_| unavailable())?;
    let transport = StreamableHttpClientTransport::with_client(
        client,
        StreamableHttpClientTransportConfig::with_uri(format!("http://127.0.0.1:{port}/mcp"))
            .auth_header(token.to_string()),
    );
    let info = ClientConfig::new(
        ClientCapabilities::default(),
        Implementation::new("Numera-CHA-Solver-MCP-bridge", env!("CARGO_PKG_VERSION")),
    );
    let mut remote = tokio::time::timeout(Duration::from_secs(10), info.serve(transport))
        .await
        .map_err(|_| unavailable())?
        .map_err(|_| unavailable())?;
    let handler = Bridge {
        peer: remote.peer().clone(),
    };
    let local = tokio::time::timeout(Duration::from_secs(15), handler.serve(stdio()))
        .await
        .map_err(|_| unavailable())?
        .map_err(|_| unavailable());
    let result = match local {
        Ok(local) => local.waiting().await.map(|_| ()).map_err(|_| unavailable()),
        Err(error) => Err(error),
    };
    tokio::time::timeout(Duration::from_secs(5), remote.close())
        .await
        .map_err(|_| unavailable())?
        .map_err(|_| unavailable())?;
    result
}
