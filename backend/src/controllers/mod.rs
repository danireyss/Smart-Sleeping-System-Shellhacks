//! HTTP layer: routes, shared state, and error responses. Handlers call
//! services only.

pub mod chat;
pub mod frontend;
pub mod readings;
pub mod sleep;
pub mod stream;
#[cfg(test)]
mod test_support;

use std::sync::Arc;

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::json;
use tokio::sync::broadcast;
use tracing::error;

use crate::domain::ScoredReading;
use crate::services::{AgentService, ReadingService, ServiceError, SleepService};

#[derive(Clone)]
pub struct AppState {
    pub readings: Arc<ReadingService>,
    pub sleep: Arc<SleepService>,
    pub agent: Arc<AgentService>,
    /// Shared token required by POST /api/chat, if set (it spends the LLM quota).
    pub chat_token: Option<Arc<str>>,
    /// Every stored reading, published by the ingest service.
    pub events: broadcast::Sender<ScoredReading>,
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/api/current", get(readings::current))
        .route("/api/readings", get(readings::readings))
        .route("/api/summary", get(readings::summary))
        .route("/api/targets", get(readings::targets))
        .route("/api/stream", get(stream::stream))
        .route("/api/sleep/start", post(sleep::start))
        .route("/api/sleep/end", post(sleep::end))
        .route("/api/sleep/current", get(sleep::current))
        .route("/api/night/latest", get(sleep::latest_night))
        .route("/api/chat", post(chat::chat))
        .fallback(frontend::serve)
        .with_state(state)
}

/// Errors returned as `{"error": "..."}` with a matching status code.
pub enum ApiError {
    NotFound(&'static str),
    BadRequest(String),
    Unauthorized,
    Conflict(String),
    Internal(ServiceError),
}

impl From<ServiceError> for ApiError {
    fn from(e: ServiceError) -> Self {
        ApiError::Internal(e)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            ApiError::NotFound(m) => (StatusCode::NOT_FOUND, m.to_string()),
            ApiError::BadRequest(m) => (StatusCode::BAD_REQUEST, m),
            ApiError::Unauthorized => {
                (StatusCode::UNAUTHORIZED, "missing or invalid chat token".to_string())
            }
            ApiError::Conflict(m) => (StatusCode::CONFLICT, m),
            ApiError::Internal(e) => {
                error!("request failed: {e}");
                (StatusCode::INTERNAL_SERVER_ERROR, "internal error".to_string())
            }
        };
        (status, Json(json!({ "error": message }))).into_response()
    }
}
