use reqwest::{Client, Method, header, redirect};
use serde_json::Value;
use std::{fmt, time::Duration};

pub const RESPONSE_LIMIT: usize = 1024 * 1024;
const IO: Duration = Duration::from_secs(10);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Failure {
    Deadline,
    Transport,
    TooLarge,
    Http,
    MalformedJson,
    InvalidEnvelope,
    Command,
}

impl fmt::Display for Failure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Deadline => "Native WebDriver command exhausted its deadline.",
            Self::Transport => "Native WebDriver transport failed; command outcome may be unknown.",
            Self::TooLarge => "Native WebDriver response exceeded its size limit.",
            Self::Http => "Native WebDriver HTTP request failed.",
            Self::MalformedJson => "Native WebDriver response was not valid UTF-8 JSON.",
            Self::InvalidEnvelope => "Native WebDriver response omitted its W3C value.",
            Self::Command => "Native WebDriver command failed.",
        })
    }
}
impl std::error::Error for Failure {}

pub fn identifier(value: &str) -> Result<String, Failure> {
    if value.is_empty()
        || value.len() > 512
        || value.chars().any(char::is_control)
        || matches!(value, "." | "..")
    {
        return Err(Failure::InvalidEnvelope);
    }
    let mut encoded = String::new();
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            encoded.push(char::from(byte));
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    Ok(encoded)
}

pub fn session_failure(status: u16, bytes: &[u8]) -> String {
    let document: Value = serde_json::from_slice(bytes).unwrap_or(Value::Null);
    let error = document["value"]["error"].as_str().unwrap_or("unknown");
    let message = document["value"]["message"]
        .as_str()
        .unwrap_or("No W3C startup diagnostic.");
    let safe = numera_cha_solver::logging::redact(&format!("HTTP {status}: {error}: {message}"));
    safe.chars().take(4096).collect()
}

pub async fn request(
    port: u16,
    method: &str,
    path: &str,
    body: Option<&Value>,
    budget: Duration,
) -> Result<Value, Failure> {
    if budget.is_zero() {
        return Err(Failure::Deadline);
    }
    let operation = async {
        // Fresh nonpooled sockets preserve at-most-one send; keep-alive must reach tauri-driver's upstream pool.
        let client = Client::builder()
            .http1_only()
            .no_proxy()
            .redirect(redirect::Policy::none())
            .retry(reqwest::retry::never())
            .pool_max_idle_per_host(0)
            .connect_timeout(budget.min(IO))
            .timeout(budget.min(IO))
            .build()
            .map_err(|_| Failure::Transport)?;
        let method = Method::from_bytes(method.as_bytes()).map_err(|_| Failure::Transport)?;
        let mut request = client
            .request(method, format!("http://127.0.0.1:{port}{path}"))
            .header(header::CONTENT_TYPE, "application/json")
            .header(header::CONNECTION, "keep-alive");
        if let Some(body) = body {
            request = request.body(serde_json::to_vec(body).map_err(|_| Failure::MalformedJson)?);
        }
        let mut response = request.send().await.map_err(|_| Failure::Transport)?;
        let status = response.status();
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(|_| Failure::Transport)? {
            if chunk.len() > RESPONSE_LIMIT.saturating_sub(bytes.len()) {
                return Err(Failure::TooLarge);
            }
            bytes.extend_from_slice(&chunk);
        }
        if !status.is_success() {
            if path == "/session" {
                numera_cha_solver::logging::event(
                    "ERROR",
                    "webdriver-session",
                    &session_failure(status.as_u16(), &bytes),
                );
            }
            return Err(Failure::Http);
        }
        let document: Value = serde_json::from_slice(&bytes).map_err(|_| Failure::MalformedJson)?;
        let value = document.get("value").ok_or(Failure::InvalidEnvelope)?;
        if value
            .as_object()
            .is_some_and(|object| object.contains_key("error"))
        {
            return Err(Failure::Command);
        }
        Ok(value.clone())
    };
    tokio::time::timeout(budget.min(IO), operation)
        .await
        .map_err(|_| Failure::Deadline)?
}
