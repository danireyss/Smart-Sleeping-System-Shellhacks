//! Shared helpers for handler tests: an app over in-memory SQLite and
//! request helpers that return (status, JSON body).

use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::Router;
use http_body_util::BodyExt;
use serde_json::Value;
use tokio::sync::broadcast;
use tower::ServiceExt;

use super::{router, AppState};
use crate::domain::{LiveEvent, Reading};
use crate::repositories::{ReadingRepository, SqliteReadingRepository, SqliteSessionRepository};
use crate::services::{AgentService, ReadingService, SleepService};

pub struct TestApp {
    pub router: Router,
    pub events: broadcast::Sender<LiveEvent>,
    pub sessions: Arc<SqliteSessionRepository>,
}

pub fn app(readings: &[Reading]) -> TestApp {
    build(readings, None)
}

pub fn app_with_chat_token(token: &str) -> TestApp {
    build(&[], Some(token.into()))
}

fn build(readings: &[Reading], chat_token: Option<Arc<str>>) -> TestApp {
    let repo = Arc::new(SqliteReadingRepository::in_memory().unwrap());
    for r in readings {
        repo.save(r, &r.flags()).unwrap();
    }
    let sessions = Arc::new(SqliteSessionRepository::in_memory().unwrap());
    let (events, _) = broadcast::channel(8);
    let readings = Arc::new(ReadingService::new(repo.clone()));
    let sleep = Arc::new(SleepService::new(sessions.clone(), repo, events.clone()));
    let state = AppState {
        agent: Arc::new(AgentService::new(None, readings.clone(), sleep.clone())),
        chat_token,
        readings,
        sleep,
        events: events.clone(),
    };
    TestApp { router: router(state), events, sessions }
}

pub async fn get(app: &TestApp, uri: &str) -> (StatusCode, Value) {
    send(app, Request::get(uri).body(Body::empty()).unwrap()).await
}

pub async fn post(app: &TestApp, uri: &str) -> (StatusCode, Value) {
    send(app, Request::post(uri).body(Body::empty()).unwrap()).await
}

async fn send(app: &TestApp, req: Request<Body>) -> (StatusCode, Value) {
    let resp = app.router.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    (status, serde_json::from_slice(&body).unwrap())
}

/// POSTs a JSON body and returns the status and the raw response text (for SSE).
pub async fn post_json_text(app: &TestApp, uri: &str, body: Value) -> (StatusCode, String) {
    send_text(app, uri, &body, None).await
}

/// Like `post_json_text`, with an optional Authorization header value.
pub async fn send_text(
    app: &TestApp,
    uri: &str,
    body: &Value,
    authorization: Option<&str>,
) -> (StatusCode, String) {
    let mut req = Request::post(uri).header("content-type", "application/json");
    if let Some(auth) = authorization {
        req = req.header("authorization", auth);
    }
    let req = req.body(Body::from(body.to_string())).unwrap();
    let resp = app.router.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    (status, String::from_utf8(bytes.to_vec()).unwrap())
}

/// GETs a path and returns the status, content type, and body text.
pub async fn get_text(app: &TestApp, uri: &str) -> (StatusCode, String, String) {
    let resp = app.router.clone().oneshot(Request::get(uri).body(Body::empty()).unwrap()).await.unwrap();
    let status = resp.status();
    let content_type = resp
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_string();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    (status, content_type, String::from_utf8_lossy(&bytes).into_owned())
}
