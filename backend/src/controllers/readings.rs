//! GET /api/current and GET /api/summary.

use axum::extract::rejection::QueryRejection;
use axum::extract::{Query, State};
use axum::Json;
use chrono::{DateTime, Utc};
use serde::Deserialize;

use super::{ApiError, AppState};
use crate::domain::summary::Summary;
use crate::domain::ScoredReading;

/// Latest reading with its flags, sub-scores, score, and band (`score` is null
/// when the reading is flagged). 404 if there are no readings yet.
pub async fn current(State(state): State<AppState>) -> Result<Json<ScoredReading>, ApiError> {
    let current = state.readings.current().await?;
    current.map(Json).ok_or(ApiError::NotFound("no readings yet"))
}

#[derive(Deserialize)]
pub struct SummaryParams {
    /// RFC 3339, e.g. `2026-09-26T02:00:00Z` (inclusive).
    start: DateTime<Utc>,
    /// RFC 3339 (exclusive).
    end: DateTime<Utc>,
}

/// Avg/min/max per metric and minutes out of range for `start <= t < end`.
pub async fn summary(
    State(state): State<AppState>,
    params: Result<Query<SummaryParams>, QueryRejection>,
) -> Result<Json<Summary>, ApiError> {
    let Query(SummaryParams { start, end }) =
        params.map_err(|e| ApiError::BadRequest(e.body_text()))?;
    if start >= end {
        return Err(ApiError::BadRequest("start must be before end".to_string()));
    }
    Ok(Json(state.readings.summary(start, end).await?))
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use axum::Router;
    use chrono::{Duration, TimeZone};
    use http_body_util::BodyExt;
    use serde_json::Value;
    use tokio::sync::broadcast;
    use tower::ServiceExt;

    use crate::controllers::{router, AppState};
    use crate::domain::reading::WARM_UP_SECS;
    use crate::domain::{Reading, ScoredReading};
    use crate::repositories::{ReadingRepository, SqliteReadingRepository};
    use crate::services::ReadingService;

    fn t0() -> chrono::DateTime<chrono::Utc> {
        chrono::Utc.with_ymd_and_hms(2026, 9, 26, 6, 0, 0).unwrap()
    }

    fn reading(secs: i64, temp: f64) -> Reading {
        Reading {
            received_at: t0() + Duration::seconds(secs),
            eco2_ppm: 450.0,
            tvoc_ppb: 5.0,
            temp_f: Some(temp),
            humidity_pct: Some(45.0),
            uptime_s: WARM_UP_SECS,
        }
    }

    fn app(readings: &[Reading]) -> (Router, broadcast::Sender<ScoredReading>) {
        let repo = Arc::new(SqliteReadingRepository::in_memory().unwrap());
        for r in readings {
            repo.save(r, &r.flags()).unwrap();
        }
        let (events, _) = broadcast::channel(8);
        let state = AppState { readings: Arc::new(ReadingService::new(repo)), events: events.clone() };
        (router(state), events)
    }

    async fn get(app: Router, uri: &str) -> (StatusCode, Value) {
        let resp = app.oneshot(Request::get(uri).body(Body::empty()).unwrap()).await.unwrap();
        let status = resp.status();
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        (status, serde_json::from_slice(&body).unwrap())
    }

    #[tokio::test]
    async fn current_is_404_without_readings() {
        let (status, body) = get(app(&[]).0, "/api/current").await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body["error"], "no readings yet");
    }

    #[tokio::test]
    async fn current_returns_latest_with_score() {
        let (status, body) = get(app(&[reading(0, 68.0), reading(10, 77.0)]).0, "/api/current").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["received_at"], "2026-09-26T06:00:10Z");
        assert_eq!(body["eco2_ppm"], 450.0);
        assert_eq!(body["temp_f"], 77.0);
        assert_eq!(body["flags"], serde_json::json!([]));
        assert_eq!(body["score"]["temp"], 30.0);
        assert_eq!(body["score"]["eco2"], 100.0);
        assert_eq!(body["score"]["band"], "fair");
    }

    #[tokio::test]
    async fn current_flagged_reading_has_null_score() {
        let warming = Reading { uptime_s: 30, temp_f: None, ..reading(0, 68.0) };
        let (_, body) = get(app(&[warming]).0, "/api/current").await;
        assert_eq!(body["flags"], serde_json::json!(["warm_up", "temp_missing"]));
        assert_eq!(body["temp_f"], Value::Null);
        assert_eq!(body["score"], Value::Null);
    }

    #[tokio::test]
    async fn summary_covers_half_open_range() {
        let readings = [reading(0, 68.0), reading(60, 77.0), reading(120, 68.0)];
        let (status, body) = get(
            app(&readings).0,
            "/api/summary?start=2026-09-26T06:00:00Z&end=2026-09-26T06:02:00Z",
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["readings"], 2);
        assert_eq!(body["valid_minutes"], 2);
        assert_eq!(body["temp_f"]["max"], 77.0);
        assert_eq!(body["temp_f"]["minutes_out_of_range"], 1);
        // (100 + 230 / 3) / 2
        assert!((body["score"]["avg"].as_f64().unwrap() - 265.0 / 3.0).abs() < 1e-9);
        assert_eq!(body["score"]["band"], "good");
    }

    #[tokio::test]
    async fn summary_with_no_readings_has_null_stats() {
        let (status, body) =
            get(app(&[]).0, "/api/summary?start=2026-09-26T06:00:00Z&end=2026-09-26T07:00:00Z").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["readings"], 0);
        assert_eq!(body["eco2_ppm"], Value::Null);
        assert_eq!(body["score"], Value::Null);
    }

    #[tokio::test]
    async fn summary_rejects_bad_params() {
        for uri in [
            "/api/summary",
            "/api/summary?start=yesterday&end=2026-09-26T07:00:00Z",
            "/api/summary?start=2026-09-26T07:00:00Z&end=2026-09-26T06:00:00Z",
        ] {
            let (status, body) = get(app(&[]).0, uri).await;
            assert_eq!(status, StatusCode::BAD_REQUEST, "{uri}");
            assert!(body["error"].is_string(), "{uri}");
        }
    }

    #[tokio::test]
    async fn stream_sends_published_readings_as_json_events() {
        let (app, events) = app(&[]);
        let resp = app
            .oneshot(Request::get("/api/stream").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.headers()["content-type"], "text/event-stream");

        events.send(ScoredReading::from(reading(0, 77.0))).unwrap();
        let mut body = resp.into_body();
        let frame = body.frame().await.unwrap().unwrap().into_data().unwrap();
        let text = String::from_utf8(frame.to_vec()).unwrap();
        let data = text
            .strip_prefix("event: reading\ndata: ")
            .and_then(|rest| rest.strip_suffix("\n\n"))
            .unwrap_or_else(|| panic!("unexpected SSE frame: {text:?}"));
        let json: Value = serde_json::from_str(data).unwrap();
        assert_eq!(json["temp_f"], 77.0);
        assert_eq!(json["score"]["temp"], 30.0);
    }
}
