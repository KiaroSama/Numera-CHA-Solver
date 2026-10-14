use crate::{
    config::Settings,
    core::contracts::{Error, Input},
    readiness::Readiness,
    sdk::Numera,
    store::TaskStore,
    tasks::TaskService,
};
use axum::{
    Json, Router,
    body::{Body, to_bytes},
    extract::{DefaultBodyLimit, Path, Request, State},
    http::{StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use base64::Engine;
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

#[derive(Clone)]
pub struct AppState {
    pub settings: Settings,
    pub sdk: Numera,
    pub tasks: Arc<TaskService>,
    pub readiness: Readiness,
    pub reviews: crate::review::ReviewStore,
    sessions: Arc<Mutex<HashMap<String, Instant>>>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Payload {
    kind: String,
    #[serde(default)]
    text: String,
    #[serde(default)]
    asset_base64: String,
    #[serde(default = "auto")]
    subtype: String,
    #[serde(default)]
    allowed_characters: String,
    #[serde(default)]
    revision: String,
}
fn auto() -> String {
    "auto".into()
}
impl Payload {
    fn input(self) -> Result<Input, Error> {
        if !matches!(self.kind.as_str(), "text" | "image" | "audio" | "browser")
            || self.text.chars().count() > 4096
            || self.subtype.chars().count() > 64
            || self.allowed_characters.chars().count() > 256
            || self.revision.chars().count() > 128
            || self.asset_base64.chars().count() > 11_184_812
        {
            return Err(Error::new(
                "invalid_input",
                "Request does not match the strict input schema.",
            ));
        }
        let asset = base64::engine::general_purpose::STANDARD
            .decode(self.asset_base64)
            .map_err(|_| Error::new("http_422", "Asset must be valid base64."))?;
        let input = Input {
            kind: self.kind,
            text: self.text,
            asset,
            subtype: self.subtype,
            allowed_characters: self.allowed_characters,
            revision: self.revision,
        };
        input.validate()?;
        Ok(input)
    }
}
fn error(status: StatusCode, code: &str, message: &str) -> Response {
    (
        status,
        Json(json!({"error":{"code":code,"message":message}})),
    )
        .into_response()
}
fn input_error(error_value: Error) -> Response {
    error(
        StatusCode::UNPROCESSABLE_ENTITY,
        &error_value.code,
        &error_value.message,
    )
}
pub fn token() -> String {
    let bytes: [u8; 32] = rand::random();
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}
fn constant_eq(a: &[u8], b: &[u8]) -> bool {
    let mut diff = a.len() ^ b.len();
    for i in 0..a.len().max(b.len()) {
        diff |= (a.get(i).copied().unwrap_or(0) ^ b.get(i).copied().unwrap_or(0)) as usize;
    }
    diff == 0
}

fn boundary_error(status: StatusCode, code: &str, message: &str, request_id: &str) -> Response {
    (
        status,
        Json(json!({"error":{"code":code,"message":message},"request_id":request_id})),
    )
        .into_response()
}
async fn boundary(State(state): State<AppState>, mut request: Request, next: Next) -> Response {
    let request_id = token();
    let host = request
        .headers()
        .get(header::HOST)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_owned();
    if !matches!(
        host.split(':').next().unwrap_or(""),
        "127.0.0.1" | "localhost" | "testserver"
    ) {
        return boundary_error(
            StatusCode::FORBIDDEN,
            "invalid_host",
            "Loopback host is required.",
            &request_id,
        );
    }
    let desktop_origin = state.settings.desktop_origin.as_deref();
    let origin_header = request
        .headers()
        .get(header::ORIGIN)
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned);
    let is_desktop = origin_header.as_deref().is_some_and(|origin| {
        desktop_origin == Some(origin)
            && matches!(origin, "tauri://localhost" | "http://tauri.localhost")
    });
    if let Some(origin) = request
        .headers()
        .get(header::ORIGIN)
        .and_then(|v| v.to_str().ok())
        && origin != format!("http://{host}")
        && !is_desktop
    {
        return boundary_error(
            StatusCode::FORBIDDEN,
            "invalid_origin",
            "Cross-origin requests are not permitted.",
            &request_id,
        );
    }
    if let Some(length) = request.headers().get(header::CONTENT_LENGTH)
        && length
            .to_str()
            .ok()
            .and_then(|s| s.parse::<usize>().ok())
            .is_none_or(|n| n > 11_190_000)
    {
        return boundary_error(
            StatusCode::PAYLOAD_TOO_LARGE,
            "input_too_large",
            "Request body exceeds its limit.",
            &request_id,
        );
    }
    if request.method() == axum::http::Method::OPTIONS && is_desktop {
        return (
            StatusCode::NO_CONTENT,
            [
                (
                    "access-control-allow-origin",
                    origin_header.as_deref().unwrap_or(""),
                ),
                ("access-control-allow-methods", "GET,POST,OPTIONS"),
                (
                    "access-control-allow-headers",
                    "authorization,content-type,idempotency-key",
                ),
            ],
        )
            .into_response();
    }
    let mut unauthorized = false;
    if request.uri().path().starts_with("/v1/") {
        let bearer = request
            .headers()
            .get(header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.strip_prefix("Bearer "))
            .unwrap_or("");
        let session = request
            .headers()
            .get(header::COOKIE)
            .and_then(|v| v.to_str().ok())
            .and_then(|s| {
                s.split(';')
                    .find_map(|p| p.trim().strip_prefix("numera_session="))
            })
            .unwrap_or("");
        let authorized = (!bearer.is_empty()
            && constant_eq(bearer.as_bytes(), state.settings.api_token.as_bytes()))
            || state
                .sessions
                .lock()
                .ok()
                .and_then(|s| s.get(session).copied())
                .is_some_and(|expiry| expiry > Instant::now());
        unauthorized = !authorized;
    }
    if matches!(request.method().as_str(), "POST" | "PUT" | "PATCH") {
        let (parts, body) = request.into_parts();
        let bytes =
            match tokio::time::timeout(Duration::from_secs(10), to_bytes(body, 11_190_000)).await {
                Ok(Ok(bytes)) => bytes,
                Ok(Err(_)) => {
                    return error(
                        StatusCode::PAYLOAD_TOO_LARGE,
                        "input_too_large",
                        "Request body exceeds its limit.",
                    );
                }
                Err(_) => {
                    return error(
                        StatusCode::REQUEST_TIMEOUT,
                        "input_timeout",
                        "Request body deadline exhausted.",
                    );
                }
            };
        request = Request::from_parts(parts, Body::from(bytes));
    }
    let mut response = if unauthorized {
        error(
            StatusCode::UNAUTHORIZED,
            "http_401",
            "Local client authorization is required.",
        )
    } else {
        next.run(request).await
    };
    if response
        .headers()
        .get(header::CONTENT_TYPE)
        .is_some_and(|v| v.to_str().unwrap_or("").starts_with("application/json"))
        && response.status().is_client_error()
    {
        let (parts, body) = response.into_parts();
        let bytes = to_bytes(body, 1024 * 1024).await.unwrap_or_default();
        let mut value:Value=serde_json::from_slice(&bytes).unwrap_or(json!({"error":{"code":"invalid_input","message":"Request does not match the strict input schema."}}));
        value["request_id"] = json!(request_id);
        let encoded = serde_json::to_vec(&value).unwrap_or_default();
        response = Response::from_parts(parts, Body::from(encoded));
        response.headers_mut().remove(header::CONTENT_LENGTH);
    }
    for (key, value) in [
        ("x-content-type-options", "nosniff"),
        (
            "content-security-policy",
            "default-src 'self'; img-src 'self' data:; frame-ancestors 'none'; base-uri 'none'; form-action 'self'",
        ),
        ("cache-control", "no-store"),
    ] {
        response.headers_mut().insert(key, value.parse().unwrap());
    }
    response
        .headers_mut()
        .insert("x-request-id", request_id.parse().unwrap());
    if is_desktop {
        response.headers_mut().insert(
            "access-control-allow-origin",
            origin_header.unwrap().parse().unwrap(),
        );
    }
    response
}
pub fn state(settings: Settings) -> Result<AppState, Error> {
    let sdk = Numera::new(settings.clone());
    let store = Arc::new(TaskStore::new(
        &settings.data_dir().join("tasks.sqlite3"),
        settings.result_ttl,
    )?);
    Ok(AppState {
        reviews: crate::review::ReviewStore::new(
            &settings.data_dir().join("reviews.sqlite3"),
            settings.result_ttl,
        )?,
        readiness: Readiness::new(settings.clone()),
        tasks: TaskService::new(sdk.clone(), store),
        sdk,
        settings,
        sessions: Arc::new(Mutex::new(HashMap::new())),
    })
}
pub fn router(state: AppState) -> Router {
    Router::new()
        .route(
            "/health/live",
            get(|| async { Json(json!({"live":true,"version":env!("CARGO_PKG_VERSION")})) }),
        )
        .route("/health/ready", get(ready))
        .route("/v1/session", post(login))
        .route("/v1/capabilities", get(capabilities))
        .route("/v1/models", get(models))
        .route("/v1/detect", post(detect))
        .route("/v1/recognize", post(recognize))
        .route("/v1/tasks", get(task_list).post(create_task))
        .route("/v1/tasks/{identifier}", get(task))
        .route("/v1/tasks/{identifier}/cancel", post(cancel))
        .route("/v1/reviews", post(crate::review_api::create))
        .route("/v1/reviews/{identifier}", get(crate::review_api::get))
        .route(
            "/v1/reviews/{identifier}/confirm",
            post(crate::review_api::confirm),
        )
        .route(
            "/v1/reviews/{identifier}/cancel",
            post(crate::review_api::cancel),
        )
        .fallback(static_file)
        .layer(DefaultBodyLimit::max(11_190_000))
        .layer(middleware::from_fn_with_state(state.clone(), boundary))
        .with_state(state)
}
async fn ready(State(state): State<AppState>) -> Json<Value> {
    Json(state.readiness.get().await)
}
async fn login(State(state): State<AppState>) -> Response {
    let session = token();
    match state.sessions.lock() {
        Ok(mut sessions) => {
            if sessions.len() >= 16 {
                sessions.clear()
            }
            sessions.insert(session.clone(), Instant::now() + Duration::from_secs(3600));
        }
        Err(_) => {
            return error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal_error",
                "Session initialization failed.",
            );
        }
    }
    let mut response = Json(json!({"authenticated":true})).into_response();
    response.headers_mut().insert(
        header::SET_COOKIE,
        format!("numera_session={session}; HttpOnly; Max-Age=3600; Path=/; SameSite=strict")
            .parse()
            .unwrap(),
    );
    response
}
async fn capabilities(State(state): State<AppState>) -> Response {
    match state.sdk.capabilities().await {
        Ok(value) => Json(value).into_response(),
        Err(error) => input_error(error),
    }
}
async fn models(State(state): State<AppState>) -> Response {
    match state.sdk.models().await {
        Ok(value) => Json(json!({"models":value,"device":state.settings.device})).into_response(),
        Err(error) => input_error(error),
    }
}
fn payload(
    value: Result<Json<Payload>, axum::extract::rejection::JsonRejection>,
) -> Result<Input, Error> {
    value
        .map_err(|_| {
            Error::new(
                "invalid_input",
                "Request does not match the strict input schema.",
            )
        })?
        .0
        .input()
}
async fn detect(
    State(state): State<AppState>,
    value: Result<Json<Payload>, axum::extract::rejection::JsonRejection>,
) -> Response {
    match payload(value).and_then(|input| state.sdk.detect(&input)) {
        Ok(value) => Json(value).into_response(),
        Err(error) => input_error(error),
    }
}
async fn recognize(
    State(state): State<AppState>,
    value: Result<Json<Payload>, axum::extract::rejection::JsonRejection>,
) -> Response {
    match payload(value) {
        Ok(input) => Json(state.sdk.recognize(input, None).await).into_response(),
        Err(error) => input_error(error),
    }
}
async fn create_task(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    value: Result<Json<Payload>, axum::extract::rejection::JsonRejection>,
) -> Response {
    let key = headers
        .get("idempotency-key")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    if key.is_empty() {
        return error(
            StatusCode::BAD_REQUEST,
            "http_400",
            "Idempotency-Key is required.",
        );
    }
    match payload(value) {
        Err(error) => input_error(error),
        Ok(input) => match state.tasks.create(key, input).await {
            Ok(value) => (StatusCode::ACCEPTED, Json(value)).into_response(),
            Err(e) => error(StatusCode::CONFLICT, "http_409", &e.message),
        },
    }
}
async fn task_list(State(state): State<AppState>) -> Response {
    match state.tasks.store.list(50) {
        Ok(tasks) => Json(json!({"tasks":tasks})).into_response(),
        Err(e) => input_error(e),
    }
}
async fn task(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    match state.tasks.store.get(&id) {
        Ok(value) => Json(value).into_response(),
        Err(_) => error(
            StatusCode::NOT_FOUND,
            "http_404",
            "Task is missing or expired.",
        ),
    }
}
async fn cancel(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    match state.tasks.cancel(&id).await {
        Ok(value) => Json(value).into_response(),
        Err(_) => error(
            StatusCode::NOT_FOUND,
            "http_404",
            "Task is missing or expired.",
        ),
    }
}
async fn static_file(request: Request) -> Response {
    if request.method() != axum::http::Method::GET && request.method() != axum::http::Method::HEAD {
        return StatusCode::METHOD_NOT_ALLOWED.into_response();
    }
    let (mime, bytes): (&str, &[u8]) = match request.uri().path() {
        "/" | "/index.html" => (
            "text/html; charset=utf-8",
            include_bytes!("../resources/web/index.html"),
        ),
        "/app.js" => (
            "text/javascript; charset=utf-8",
            include_bytes!("../resources/web/app.js"),
        ),
        "/app.css" => (
            "text/css; charset=utf-8",
            include_bytes!("../resources/web/app.css"),
        ),
        "/logo.png" => ("image/png", include_bytes!("../resources/web/logo.png")),
        _ => return StatusCode::NOT_FOUND.into_response(),
    };
    ([(header::CONTENT_TYPE, mime)], bytes).into_response()
}
