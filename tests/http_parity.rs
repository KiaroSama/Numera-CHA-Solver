use base64::Engine;
use numera_cha_solver::{api, config::Settings};
use serde_json::{Value, json};
use std::time::Duration;

#[tokio::test]
async fn http_matches_source_schema_security_and_large_input_contracts() {
    let fixtures: Value = serde_json::from_str(include_str!("fixtures/parity/http.json")).unwrap();
    let root = tempfile::tempdir().unwrap();
    let mut settings = Settings::new(root.path().into());
    settings.api_token = "owned-oracle-only".into();
    let state = api::state(settings).unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let (stop, shutdown) = tokio::sync::oneshot::channel();
    let router = api::router(state.clone());
    let server = tokio::spawn(async move {
        axum::serve(listener, router)
            .with_graceful_shutdown(async {
                let _ = shutdown.await;
            })
            .await
            .unwrap()
    });
    let result=tokio::time::timeout(Duration::from_secs(15),async{
        let client=reqwest::Client::builder().no_proxy().timeout(Duration::from_secs(3)).build().unwrap();let mut differences=Vec::new();
        for case in fixtures["cases"].as_array().unwrap(){
            let mut request=client.request(case["method"].as_str().unwrap().parse().unwrap(),format!("http://{address}{}",case["path"].as_str().unwrap()));
            if case["authorized"]==true{request=request.bearer_auth("owned-oracle-only")}
            if let Some(headers)=case["headers"].as_object(){for(key,value)in headers{request=request.header(key,value.as_str().unwrap())}}
            let mut body=case.get("body").cloned();
            if let Some(count)=case["text_count"].as_u64(){body=Some(json!({"kind":"text","text":"é".repeat(count as usize)}))}
            if let Some(count)=case["asset_bytes"].as_u64(){body=Some(json!({"kind":"audio","asset_base64":base64::engine::general_purpose::STANDARD.encode(vec![0;count as usize])}))}
            if let Some(body)=body{request=request.header("content-type","application/json").body(body.to_string())}
            let response=request.send().await.unwrap();
            let status=response.status().as_u16();let mut headers=serde_json::Map::new();
            for key in ["x-content-type-options","content-security-policy","cache-control","x-request-id"]{if let Some(value)=response.headers().get(key){headers.insert(key.into(),json!(if key=="x-request-id"{"<request-id>"}else{value.to_str().unwrap()}));}}
            let mut body:Value=serde_json::from_slice(&response.bytes().await.unwrap()).unwrap();if body.get("request_id").is_some(){body["request_id"]=json!("<request-id>")}
            let actual=json!({"status":status,"body":body,"headers":headers});if actual!=case["expected"]{differences.push(format!("{}: actual={actual}, expected={}",case["name"],case["expected"]))}
        }
        differences
    }).await;
    let _ = stop.send(());
    tokio::time::timeout(Duration::from_secs(5), server)
        .await
        .unwrap()
        .unwrap();
    state.tasks.close().await.expect("durable task shutdown");
    state.readiness.close().await;
    let differences = result.unwrap();
    assert!(differences.is_empty(), "{}", differences.join("\n"));
}
