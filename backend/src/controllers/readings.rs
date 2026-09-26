//! GET /api/current, GET /api/readings, and GET /api/summary.

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
pub struct RangeParams {
    /// RFC 3339, e.g. `2026-09-26T02:00:00Z` (inclusive).
    start: DateTime<Utc>,
    /// RFC 3339 (exclusive).
    end: DateTime<Utc>,
}

type RangeQuery = Result<Query<RangeParams>, QueryRejection>;

fn parse_range(params: RangeQuery) -> Result<(DateTime<Utc>, DateTime<Utc>), ApiError> {
    let Query(RangeParams { start, end }) =
        params.map_err(|e| ApiError::BadRequest(e.body_text()))?;
    if start >= end {
        return Err(ApiError::BadRequest("start must be before end".to_string()));
    }
    Ok((start, end))
}

/// Every reading with `start <= t < end`, oldest first, each with its flags and
/// score (same shape as /api/current). For charts.
pub async fn readings(
    State(state): State<AppState>,
    params: RangeQuery,
) -> Result<Json<Vec<ScoredReading>>, ApiError> {
    let (start, end) = parse_range(params)?;
    Ok(Json(state.readings.readings(start, end).await?))
}

/// Avg/min/max per metric and minutes out of range for `start <= t < end`.
pub async fn summary(
    State(state): State<AppState>,
    params: RangeQuery,
) -> Result<Json<Summary>, ApiError> {
    let (start, end) = parse_range(params)?;
    Ok(Json(state.readings.summary(start, end).await?))
}

#[cfg(test)]
mod tests {
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use chrono::{Duration, TimeZone};
    use http_body_util::BodyExt;
    use serde_json::Value;
    use tower::ServiceExt;

    use crate::controllers::test_support::{app, get};
    use crate::domain::reading::WARM_UP_SECS;
    use crate::domain::{Reading, ScoredReading};

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

    #[tokio::test]
    async fn current_is_404_without_readings() {
        let (status, body) = get(&app(&[]), "/api/current").await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body["error"], "no readings yet");
    }

    #[tokio::test]
    async fn current_returns_latest_with_score() {
        let app = app(&[reading(0, 68.0), reading(10, 77.0)]);
        let (status, body) = get(&app, "/api/current").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["received_at"], "2026-09-26T06:00:10Z");
        assert_eq!(body["eco2_ppm"], serde_json::json!(450));
        assert_eq!(body["tvoc_ppb"], serde_json::json!(5));
        assert_eq!(body["temp_f"], 77.0);
        assert_eq!(body["flags"], serde_json::json!([]));
        assert_eq!(body["score"]["temp"], 30.0);
        assert_eq!(body["score"]["eco2"], 100.0);
        assert_eq!(body["score"]["band"], "fair");
    }

    #[tokio::test]
    async fn current_flagged_reading_has_null_score() {
        let warming = Reading { uptime_s: 30, temp_f: None, ..reading(0, 68.0) };
        let (_, body) = get(&app(&[warming]), "/api/current").await;
        assert_eq!(body["flags"], serde_json::json!(["warm_up", "temp_missing"]));
        assert_eq!(body["temp_f"], Value::Null);
        assert_eq!(body["score"], Value::Null);
    }

    #[tokio::test]
    async fn summary_covers_half_open_range() {
        let readings = [reading(0, 68.0), reading(60, 76.0), reading(120, 68.0)];
        let (status, body) = get(&app(&readings),
            "/api/summary?start=2026-09-26T06:00:00Z&end=2026-09-26T06:02:00Z",
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["readings"], 2);
        assert_eq!(body["valid_minutes"], 2);
        assert_eq!(body["temp_f"]["max"], 76.0);
        assert_eq!(body["temp_f"]["minutes_out_of_range"], 1);
        assert_eq!(body["eco2_ppm"]["avg"], serde_json::json!(450));
        // Totals 100 and (100 + 40 + 100) / 3 = 80
        assert_eq!(body["score"]["avg"], 90.0);
        assert_eq!(body["score"]["band"], "great");
    }

    #[tokio::test]
    async fn readings_returns_range_with_scores_oldest_first() {
        let warming = Reading { uptime_s: 30, ..reading(60, 68.0) };
        let readings = [reading(120, 76.64), warming, reading(0, 68.0), reading(180, 68.0)];
        let (status, body) = get(&app(&readings),
            "/api/readings?start=2026-09-26T06:00:00Z&end=2026-09-26T06:03:00Z",
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let rows = body.as_array().unwrap();
        let times: Vec<_> = rows.iter().map(|r| r["received_at"].as_str().unwrap()).collect();
        assert_eq!(times, ["2026-09-26T06:00:00Z", "2026-09-26T06:01:00Z", "2026-09-26T06:02:00Z"]);
        assert_eq!(rows[0]["score"]["total"], 100.0);
        assert_eq!(rows[1]["flags"], serde_json::json!(["warm_up"]));
        assert_eq!(rows[1]["score"], Value::Null);
        assert_eq!(rows[2]["temp_f"], 76.6);
        assert_eq!(rows[2]["score"]["temp"], 33.6);
        assert_eq!(rows[2]["score"]["total"], 77.9);
    }

    #[tokio::test]
    async fn readings_rejects_bad_range() {
        let uri = "/api/readings?start=2026-09-26T07:00:00Z&end=2026-09-26T06:00:00Z";
        let (status, _) = get(&app(&[]), uri).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn summary_with_no_readings_has_null_stats() {
        let (status, body) =
            get(&app(&[]), "/api/summary?start=2026-09-26T06:00:00Z&end=2026-09-26T07:00:00Z").await;
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
            let (status, body) = get(&app(&[]), uri).await;
            assert_eq!(status, StatusCode::BAD_REQUEST, "{uri}");
            assert!(body["error"].is_string(), "{uri}");
        }
    }

    #[tokio::test]
    async fn stream_sends_published_readings_as_json_events() {
        let app = app(&[]);
        let resp = app
            .router
            .clone()
            .oneshot(Request::get("/api/stream").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.headers()["content-type"], "text/event-stream");

        app.events.send(ScoredReading::from(reading(0, 77.0))).unwrap();
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
