use crate::api::AppState;
use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::Deserialize;
use serde_json::json;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Create {
    revision: String,
    suggestion: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Decision {
    revision: String,
    answer: Option<String>,
    human_confirmed: bool,
}
fn response(result: Result<serde_json::Value, crate::core::contracts::Error>) -> Response {
    match result {
        Ok(value) => Json(value).into_response(),
        Err(error) => (StatusCode::CONFLICT, Json(json!({"error":error}))).into_response(),
    }
}
pub async fn create(State(state): State<AppState>, Json(input): Json<Create>) -> Response {
    response(state.reviews.create(&input.revision, &input.suggestion))
}
pub async fn get(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    response(state.reviews.get(&id))
}
pub async fn confirm(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<Decision>,
) -> Response {
    if !input.human_confirmed || input.answer.is_none() {
        return (StatusCode::BAD_REQUEST, Json(json!({"error":{"code":"human_review_required","message":"A person must explicitly review and confirm the answer."}}))).into_response();
    }
    response(
        state
            .reviews
            .finish(&id, &input.revision, input.answer.as_deref()),
    )
}
pub async fn cancel(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<Decision>,
) -> Response {
    response(state.reviews.finish(&id, &input.revision, None))
}
