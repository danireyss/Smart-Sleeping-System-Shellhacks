//! Sleep sessions and nightly reports:
//! POST /api/sleep/start, POST /api/sleep/end, GET /api/sleep/current,
//! GET /api/night/latest.

use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;

use super::{ApiError, AppState};
use crate::domain::sleep::{NightReport, SleepSession, StartOutcome};

/// 201 with the new session, or 409 if a session is already open.
pub async fn start(
    State(state): State<AppState>,
) -> Result<(StatusCode, Json<SleepSession>), ApiError> {
    match state.sleep.start().await? {
        StartOutcome::Started(session) => Ok((StatusCode::CREATED, Json(session))),
        StartOutcome::AlreadyOpen(open) => Err(ApiError::Conflict(format!(
            "a sleep session is already open (started {})",
            open.started_at.format("%Y-%m-%dT%H:%M:%SZ")
        ))),
    }
}

/// 200 with the closed session, or 409 if no session is open.
pub async fn end(State(state): State<AppState>) -> Result<Json<SleepSession>, ApiError> {
    let ended = state.sleep.end().await?;
    ended.map(Json).ok_or_else(|| ApiError::Conflict("no sleep session is open".to_string()))
}

/// The open session, or `null`.
pub async fn current(State(state): State<AppState>) -> Result<Json<Option<SleepSession>>, ApiError> {
    Ok(Json(state.sleep.current().await?))
}

/// Report for the most recently ended session. 404 if no session has ended.
pub async fn latest_night(State(state): State<AppState>) -> Result<Json<NightReport>, ApiError> {
    let report = state.sleep.latest_night().await?;
    report.map(Json).ok_or(ApiError::NotFound("no finished sleep session yet"))
}

#[cfg(test)]
mod tests {
    use axum::http::StatusCode;
    use chrono::{DateTime, Duration, TimeZone, Utc};
    use serde_json::{json, Value};

    use crate::controllers::test_support::{app, get, post};
    use crate::domain::reading::WARM_UP_SECS;
    use crate::domain::Reading;
    use crate::repositories::SessionRepository;

    #[tokio::test]
    async fn session_lifecycle() {
        let app = app(&[]);
        assert_eq!(get(&app, "/api/sleep/current").await, (StatusCode::OK, Value::Null));

        let (status, started) = post(&app, "/api/sleep/start").await;
        assert_eq!(status, StatusCode::CREATED);
        assert_eq!(started["ended_at"], Value::Null);
        assert_eq!(get(&app, "/api/sleep/current").await, (StatusCode::OK, started.clone()));

        let (status, body) = post(&app, "/api/sleep/start").await;
        assert_eq!(status, StatusCode::CONFLICT);
        assert!(body["error"].as_str().unwrap().contains("already open"));

        let (status, ended) = post(&app, "/api/sleep/end").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(ended["id"], started["id"]);
        assert_eq!(ended["started_at"], started["started_at"]);
        assert!(ended["ended_at"].is_string());
        assert_eq!(get(&app, "/api/sleep/current").await, (StatusCode::OK, Value::Null));

        let (status, body) = post(&app, "/api/sleep/end").await;
        assert_eq!(status, StatusCode::CONFLICT);
        assert_eq!(body["error"], "no sleep session is open");
    }

    fn t(hour: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 9, 26, hour, 0, 0).unwrap()
    }

    fn reading(at: DateTime<Utc>, temp: f64) -> Reading {
        Reading {
            received_at: at,
            eco2_ppm: 477.4,
            tvoc_ppb: 11.0,
            temp_f: Some(temp),
            humidity_pct: Some(49.8),
            uptime_s: WARM_UP_SECS,
        }
    }

    #[tokio::test]
    async fn latest_night_is_404_until_a_session_ends() {
        let app = app(&[]);
        let (status, body) = get(&app, "/api/night/latest").await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body["error"], "no finished sleep session yet");

        app.sessions.start(t(2)).unwrap(); // still open
        assert_eq!(get(&app, "/api/night/latest").await.0, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn latest_night_reports_the_last_ended_session() {
        // Readings every minute from 02:00 to 04:00; the session covers 02:30–03:30.
        let readings: Vec<_> =
            (0..120).map(|m| reading(t(2) + Duration::minutes(m), 76.64)).collect();
        let app = app(&readings);
        app.sessions.start(t(2) + Duration::minutes(30)).unwrap();
        app.sessions.end(t(3) + Duration::minutes(30)).unwrap();

        let (status, night) = get(&app, "/api/night/latest").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(night["started_at"], "2026-09-26T02:30:00Z");
        assert_eq!(night["ended_at"], "2026-09-26T03:30:00Z");
        assert_eq!(night["duration_minutes"], 60);
        assert_eq!(night["short_session"], false);
        assert_eq!(night["readings"], 60);
        assert_eq!(night["valid_minutes"], 60);
        assert_eq!(night["completeness_pct"], 100.0);
        assert_eq!(night["incomplete"], false);
        assert_eq!(night["score"], 77.9);
        assert_eq!(night["band"], "fair");
        assert_eq!(night["eco2_ppm"]["avg"], json!(477));
        assert_eq!(night["temp_f"]["avg"], 76.6);
        assert_eq!(night["temp_f"]["minutes_out_of_range"], 60);
        assert_eq!(night["lowest_metric"], json!({"metric": "temp", "avg_score": 33.6}));
    }
}
