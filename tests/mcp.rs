use numera_cha_solver::{config::Settings, mcp, review::ReviewStore, sdk::Numera};
use rmcp::{
    ServiceExt,
    model::{CallToolRequestParams, ClientCapabilities, ClientConfig, Implementation},
    transport::{
        StreamableHttpClientTransport, streamable_http_client::StreamableHttpClientTransportConfig,
    },
};
use serde_json::{Value, json};
use std::time::Duration;
use tokio_util::sync::CancellationToken;

#[test]
fn port_bounds_and_exact_registration_contract() {
    assert_eq!(mcp::port(None).unwrap(), 1583);
    assert_eq!(mcp::port(Some("1900")).unwrap(), 1900);
    for value in ["0", "65536", "-1", "http://example.com", ""] {
        assert!(mcp::port(Some(value)).is_err());
    }
}
#[tokio::test]
async fn authenticated_protocol_tools_and_human_boundary() {
    let root = tempfile::tempdir().unwrap();
    let settings = Settings::new(root.path().into());
    let sdk = Numera::new(settings.clone());
    let tools = mcp::tools::Tools {
        sdk: sdk.clone(),
        reviews: ReviewStore::new(&root.path().join("reviews.sqlite3"), 60).unwrap(),
    };
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let token = "owned-mcp-synthetic-credential";
    let cancel = CancellationToken::new();
    let stop = cancel.clone();
    let router = mcp::transport::router(tools, token.into(), port, cancel.clone());
    let server = tokio::spawn(async move {
        axum::serve(listener, router)
            .with_graceful_shutdown(async move { stop.cancelled().await })
            .await
            .unwrap();
    });
    let client = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(5))
        .build()
        .unwrap();
    let url = format!("http://127.0.0.1:{port}/mcp");
    for (auth, origin, host, status) in [
        (false, None, None, 401),
        (true, Some("https://hostile.invalid"), None, 403),
        (true, None, Some("hostile.invalid"), 403),
    ] {
        let mut request = client.post(&url).body("{}");
        if auth {
            request = request.bearer_auth(token)
        }
        if let Some(origin) = origin {
            request = request.header("origin", origin)
        }
        if let Some(host) = host {
            request = request.header("host", host)
        }
        assert_eq!(request.send().await.unwrap().status().as_u16(), status);
    }
    let transport = StreamableHttpClientTransport::with_client(
        client,
        StreamableHttpClientTransportConfig::with_uri(url).auth_header(token.to_string()),
    );
    let info = ClientConfig::new(
        ClientCapabilities::default(),
        Implementation::new("owned-test", "1"),
    );
    let mut peer = tokio::time::timeout(Duration::from_secs(5), info.serve(transport))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        peer.peer_info()
            .unwrap()
            .server_info
            .as_ref()
            .expect("server identity supplied by initialize")
            .name,
        "Numera-CHA-Solver-MCP"
    );
    eprintln!("MCP checkpoint: authenticated initialize and boundary rejection passed.");
    let catalog = peer.list_tools(None).await.unwrap();
    assert!(catalog.tools.iter().any(|t| t.name == "recognize"));
    assert!(
        !catalog
            .tools
            .iter()
            .any(|t| t.name.contains("confirm") || t.name.contains("submit"))
    );
    let response = peer
        .call_tool(
            CallToolRequestParams::new("recognize").with_arguments(
                json!({"kind":"text","text":"12 * 3 = ?"})
                    .as_object()
                    .unwrap()
                    .clone(),
            ),
        )
        .await
        .unwrap();
    let value: Value = response.structured_content.unwrap();
    assert_eq!(value["answer"], "36");
    assert_eq!(value["server_verified"], false);
    eprintln!("MCP checkpoint: tool catalog and arithmetic contract passed.");
    use base64::Engine;
    let large = base64::engine::general_purpose::STANDARD.encode(vec![0u8; 3_200_000]);
    let large_result = peer
        .call_tool(
            CallToolRequestParams::new("recognize").with_arguments(
                json!({"kind":"image","asset_base64":large})
                    .as_object()
                    .unwrap()
                    .clone(),
            ),
        )
        .await
        .unwrap();
    assert_eq!(
        large_result.structured_content.unwrap()["outcome"],
        "failed"
    );
    eprintln!("MCP checkpoint: valid request above4MiB reached bounded decoding.");
    let oversized = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(5))
        .build()
        .unwrap()
        .post(format!("http://127.0.0.1:{port}/mcp"))
        .bearer_auth(token)
        .body(vec![b' '; 11_190_001])
        .send()
        .await
        .unwrap();
    assert_eq!(oversized.status().as_u16(), 413);
    let invalid = peer
        .call_tool(
            CallToolRequestParams::new("recognize").with_arguments(
                json!({"kind":"browser","url":"https://example.com"})
                    .as_object()
                    .unwrap()
                    .clone(),
            ),
        )
        .await
        .expect("SDK parameter rejection is a tool error result");
    assert_eq!(invalid.is_error, Some(true));
    assert!(invalid.structured_content.is_none());
    assert!(
        serde_json::to_value(&invalid.content)
            .unwrap()
            .to_string()
            .contains("unknown field `url`")
    );
    let unsupported = peer
        .call_tool(
            CallToolRequestParams::new("recognize")
                .with_arguments(json!({"kind":"browser"}).as_object().unwrap().clone()),
        )
        .await;
    assert!(unsupported.is_err());
    eprintln!("MCP checkpoint: oversized body and unsupported input rejected.");
    peer.close().await.unwrap();
    cancel.cancel();
    tokio::time::timeout(Duration::from_secs(5), server)
        .await
        .unwrap()
        .unwrap();
    sdk.close().await;
}
