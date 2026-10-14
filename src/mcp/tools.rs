use crate::{core::contracts::Input, review::ReviewStore, sdk::Numera};
use base64::Engine;
use rmcp::{
    ErrorData, RoleServer, ServerHandler,
    handler::server::wrapper::Parameters,
    model::{CallToolResult, ContentBlock, Implementation, ServerCapabilities, ServerConfig},
    schemars,
    service::RequestContext,
    tool, tool_handler, tool_router,
};
use serde::Deserialize;
use serde_json::{Value, json};

#[derive(Clone)]
pub struct Tools {
    pub sdk: Numera,
    pub reviews: ReviewStore,
}
#[derive(Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Recognition {
    /// text or image; local licensed input only, never a remote URL.
    pub kind: String,
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub asset_base64: String,
    #[serde(default)]
    pub allowed_characters: String,
    #[serde(default)]
    pub revision: String,
}
#[derive(Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Review {
    pub revision: String,
    pub suggestion: String,
}
#[derive(Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Identity {
    pub id: String,
}
#[derive(Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Cancellation {
    pub id: String,
    pub revision: String,
}
fn result(value: Value) -> CallToolResult {
    let mut output = CallToolResult::success(vec![ContentBlock::text(value.to_string())]);
    output.structured_content = Some(value);
    output
}
fn error(value: crate::core::contracts::Error) -> ErrorData {
    ErrorData::invalid_params(value.message, Some(json!({"code":value.code})))
}
#[tool_router]
impl Tools {
    #[tool(
        description = "Inspect actual local capabilities and explicit blockers; registry entries are not provider solving support.",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    pub async fn capabilities(&self) -> Result<CallToolResult, ErrorData> {
        self.sdk.capabilities().await.map(result).map_err(error)
    }
    #[tool(
        description = "Inspect immutable local model assets; never downloads weights.",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    pub async fn models(&self) -> Result<CallToolResult, ErrorData> {
        self.sdk
            .models()
            .await
            .map(|models| result(json!({"models":models})))
            .map_err(error)
    }
    #[tool(
        description = "Inspect local prerequisites without logging credentials; inventory is not inference evidence.",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    pub async fn doctor(&self) -> Result<CallToolResult, ErrorData> {
        self.sdk.doctor().await.map(result).map_err(error)
    }
    #[tool(
        description = "Recognize bounded licensed local text/image. Never submits to sites or bypasses CAPTCHA; answer is not server acceptance.",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    pub async fn recognize(
        &self,
        Parameters(request): Parameters<Recognition>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        if !matches!(request.kind.as_str(), "text" | "image")
            || request.asset_base64.len() > 11_184_812
        {
            return Err(ErrorData::invalid_params(
                "Only bounded licensed local text or image is accepted.",
                None,
            ));
        }
        let asset = base64::engine::general_purpose::STANDARD
            .decode(request.asset_base64)
            .map_err(|_| ErrorData::invalid_params("Asset must be valid base64.", None))?;
        let input = Input {
            kind: request.kind,
            text: request.text,
            asset,
            subtype: "auto".into(),
            allowed_characters: request.allowed_characters,
            revision: request.revision,
        };
        input.validate().map_err(error)?;
        let operation = self.sdk.recognize(input, None);
        tokio::select! {
            value=operation => serde_json::to_value(value).map(result).map_err(|_|ErrorData::internal_error("Result serialization failed.",None)),
            _=context.ct.cancelled()=>Err(ErrorData::internal_error("Recognition cancelled; owned work is draining.",None)),
        }
    }
    #[tool(
        description = "Create a pending manual review bound to an observation revision. An agent cannot confirm it or claim site acceptance.",
        annotations(read_only_hint = false, open_world_hint = false)
    )]
    pub async fn create_review(
        &self,
        Parameters(request): Parameters<Review>,
    ) -> Result<CallToolResult, ErrorData> {
        self.reviews
            .create(&request.revision, &request.suggestion)
            .map(result)
            .map_err(error)
    }
    #[tool(
        description = "Read manual review status; confirmed means user attestation, not third-party server verification.",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    pub async fn get_review(
        &self,
        Parameters(request): Parameters<Identity>,
    ) -> Result<CallToolResult, ErrorData> {
        self.reviews.get(&request.id).map(result).map_err(error)
    }
    #[tool(
        description = "Cancel a pending review at its exact revision; no confirmation or external action.",
        annotations(
            read_only_hint = false,
            destructive_hint = false,
            open_world_hint = false
        )
    )]
    pub async fn cancel_review(
        &self,
        Parameters(request): Parameters<Cancellation>,
    ) -> Result<CallToolResult, ErrorData> {
        self.reviews
            .finish(&request.id, &request.revision, None)
            .map(result)
            .map_err(error)
    }
}
#[tool_handler]
impl ServerHandler for Tools {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("Numera-CHA-Solver-MCP",env!("CARGO_PKG_VERSION")))
            .with_instructions("Local licensed recognition and human handoff only. Never bypass protections or impersonate confirmation; server_verified stays independent.")
    }
}
