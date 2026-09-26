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
use crate::domain::{Reading, ScoredReading};
use crate::repositories::{ReadingRepository, SqliteReadingRepository, SqliteSessionRepository};
use crate::services::{ReadingService, SleepService};

pub struct TestApp {
    pub router: Router,
    pub events: broadcast::Sender<ScoredReading>,
    pub sessions: Arc<SqliteSessionRepository>,
}

pub fn app(readings: &[Reading]) -> TestApp {
    let repo = Arc::new(SqliteReadingRepository::in_memory().unwrap());
    for r in readings {
        repo.save(r, &r.flags()).unwrap();
    }
    let sessions = Arc::new(SqliteSessionRepository::in_memory().unwrap());
    let (events, _) = broadcast::channel(8);
    let state = AppState {
        readings: Arc::new(ReadingService::new(repo.clone())),
        sleep: Arc::new(SleepService::new(sessions.clone(), repo)),
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
