//! Sleep sessions (started and ended by the user) and the nightly report for a
//! finished session. Pure types and functions, no I/O.
//!
//! Nightly score = average score of the valid readings in `[started_at, ended_at)`
//! (equal to averaging minutes, since the interval is constant). Completeness =
//! distinct minutes with a valid reading ÷ session minutes; under 60% is
//! "incomplete". Sessions under 1 hour are marked short.

use chrono::{DateTime, Utc};
use serde::Serialize;

use super::round::{self, round1};
use super::scoring::{band, Band};
use super::summary::{summarize, MetricStats};
use super::Reading;

pub const INCOMPLETE_BELOW_PCT: f64 = 60.0;
pub const SHORT_SESSION_SECS: i64 = 3600;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SleepSession {
    pub id: i64,
    pub started_at: DateTime<Utc>,
    /// `None` while the session is open.
    pub ended_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum StartOutcome {
    Started(SleepSession),
    /// A session was already open; nothing was created.
    AlreadyOpen(SleepSession),
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct NightReport {
    pub session_id: i64,
    pub started_at: DateTime<Utc>,
    pub ended_at: DateTime<Utc>,
    /// Session length rounded to whole minutes (at least 1).
    pub duration_minutes: i64,
    /// True when the session is under 1 hour.
    pub short_session: bool,
    /// Average score of valid readings; `None` if there are none.
    #[serde(serialize_with = "round::tenths_opt")]
    pub score: Option<f64>,
    pub band: Option<Band>,
    /// Distinct minutes with a valid reading ÷ `duration_minutes`, as a percentage.
    #[serde(serialize_with = "round::tenths")]
    pub completeness_pct: f64,
    pub incomplete: bool,
    pub readings: usize,
    pub valid_readings: usize,
    pub valid_minutes: usize,
    pub eco2_ppm: Option<MetricStats>,
    pub temp_f: Option<MetricStats>,
    pub humidity_pct: Option<MetricStats>,
    /// Metric with the lowest average sub-score; `None` if there are no valid
    /// readings or every metric averaged 100.
    pub lowest_metric: Option<LowestMetric>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct LowestMetric {
    /// "eco2", "temp", or "humidity" (same names as the score fields).
    pub metric: &'static str,
    #[serde(serialize_with = "round::tenths")]
    pub avg_score: f64,
}

/// Builds the report for a session that ended at `ended_at`. `readings` should
/// be those in `[started_at, ended_at)`.
pub fn night_report(
    session_id: i64,
    started_at: DateTime<Utc>,
    ended_at: DateTime<Utc>,
    readings: &[Reading],
) -> NightReport {
    let summary = summarize(started_at, ended_at, readings);
    let secs = (ended_at - started_at).num_seconds().max(0);
    let duration_minutes = ((secs as f64 / 60.0).round() as i64).max(1);
    let completeness_pct =
        (summary.valid_minutes as f64 / duration_minutes as f64 * 100.0).min(100.0);
    let score = summary.score.map(|s| round1(s.avg));

    NightReport {
        session_id,
        started_at,
        ended_at,
        duration_minutes,
        short_session: secs < SHORT_SESSION_SECS,
        score,
        band: score.map(band),
        completeness_pct,
        incomplete: round1(completeness_pct) < INCOMPLETE_BELOW_PCT,
        readings: summary.readings,
        valid_readings: summary.valid_readings,
        valid_minutes: summary.valid_minutes,
        lowest_metric: lowest_metric([
            ("eco2", summary.eco2_ppm),
            ("temp", summary.temp_f),
            ("humidity", summary.humidity_pct),
        ]),
        eco2_ppm: summary.eco2_ppm,
        temp_f: summary.temp_f,
        humidity_pct: summary.humidity_pct,
    }
}

/// Lowest rounded average sub-score; ties go to the earlier metric.
fn lowest_metric(metrics: [(&'static str, Option<MetricStats>); 3]) -> Option<LowestMetric> {
    let mut lowest: Option<LowestMetric> = None;
    for (metric, stats) in metrics {
        let Some(stats) = stats else { continue };
        let avg_score = round1(stats.avg_score);
        match lowest {
            Some(l) if l.avg_score <= avg_score => {}
            _ => lowest = Some(LowestMetric { metric, avg_score }),
        }
    }
    lowest.filter(|l| l.avg_score < 100.0)
}

#[cfg(test)]
mod tests {
    use chrono::{Duration, TimeZone};

    use super::*;
    use crate::domain::reading::WARM_UP_SECS;

    fn start() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 9, 26, 3, 0, 0).unwrap()
    }

    fn at(secs: i64, eco2: f64, temp: f64, humidity: f64) -> Reading {
        Reading {
            received_at: start() + Duration::seconds(secs),
            eco2_ppm: eco2,
            tvoc_ppb: 0.0,
            temp_f: Some(temp),
            humidity_pct: Some(humidity),
            uptime_s: WARM_UP_SECS,
        }
    }

    /// One reading per minute for `minutes` minutes.
    fn every_minute(minutes: i64, temp: f64) -> Vec<Reading> {
        (0..minutes).map(|m| at(m * 60, 600.0, temp, 45.0)).collect()
    }

    #[test]
    fn full_night_report() {
        let end = start() + Duration::hours(8);
        let r = night_report(1, start(), end, &every_minute(480, 76.64));
        assert_eq!(r.duration_minutes, 480);
        assert!(!r.short_session);
        assert_eq!(r.score, Some(78.0)); // 76.64 shown as 76.6: (100 + 34 + 100) / 3
        assert_eq!(r.band, Some(Band::Fair));
        assert_eq!(r.completeness_pct, 100.0);
        assert!(!r.incomplete);
        assert_eq!(r.valid_minutes, 480);
        assert_eq!(r.temp_f.unwrap().minutes_out_of_range, 480);
        assert_eq!(r.lowest_metric, Some(LowestMetric { metric: "temp", avg_score: 34.0 }));
    }

    #[test]
    fn completeness_counts_distinct_valid_minutes() {
        // 10 s interval: six readings per minute for 30 of 100 minutes.
        let readings: Vec<_> = (0..30 * 6).map(|i| at(i * 10, 600.0, 68.0, 45.0)).collect();
        let r = night_report(1, start(), start() + Duration::minutes(100), &readings);
        assert_eq!(r.valid_readings, 180);
        assert_eq!(r.valid_minutes, 30);
        assert_eq!(r.completeness_pct, 30.0);
        assert!(r.incomplete);
    }

    #[test]
    fn flagged_readings_do_not_count_toward_completeness() {
        let mut readings = every_minute(100, 68.0);
        for r in readings.iter_mut().take(41) {
            r.uptime_s = 0; // warm-up
        }
        let r = night_report(1, start(), start() + Duration::minutes(100), &readings);
        assert_eq!(r.readings, 100);
        assert_eq!(r.valid_minutes, 59);
        assert!(r.incomplete); // 59% < 60%

        readings[40].uptime_s = WARM_UP_SECS;
        let r = night_report(1, start(), start() + Duration::minutes(100), &readings);
        assert_eq!(r.completeness_pct, 60.0);
        assert!(!r.incomplete); // exactly 60% is complete
    }

    #[test]
    fn short_session_is_flagged() {
        let r = night_report(1, start(), start() + Duration::minutes(59), &every_minute(59, 68.0));
        assert!(r.short_session);
        let r = night_report(1, start(), start() + Duration::minutes(60), &every_minute(60, 68.0));
        assert!(!r.short_session);
    }

    #[test]
    fn empty_session_has_no_score() {
        let r = night_report(1, start(), start() + Duration::hours(7), &[]);
        assert_eq!((r.score, r.band), (None, None));
        assert_eq!(r.completeness_pct, 0.0);
        assert!(r.incomplete);
        assert_eq!(r.lowest_metric, None);
        assert!(r.eco2_ppm.is_none());
    }

    #[test]
    fn lowest_metric_is_none_when_everything_is_on_target() {
        let r = night_report(1, start(), start() + Duration::hours(1), &every_minute(60, 68.0));
        assert_eq!(r.score, Some(100.0));
        assert_eq!(r.band, Some(Band::Great));
        assert_eq!(r.lowest_metric, None);
    }

    #[test]
    fn lowest_metric_picks_minimum_average_sub_score() {
        // eCO2 1400 -> 50, temp 72.5 -> 75, humidity 45 -> 100
        let readings: Vec<_> = (0..60).map(|m| at(m * 60, 1400.0, 72.5, 45.0)).collect();
        let r = night_report(1, start(), start() + Duration::hours(1), &readings);
        assert_eq!(r.lowest_metric, Some(LowestMetric { metric: "eco2", avg_score: 50.0 }));
        assert_eq!(r.score, Some(75.0));
    }

    #[test]
    fn serializes_with_api_rounding() {
        let r = night_report(7, start(), start() + Duration::hours(8), &every_minute(480, 76.64));
        let json = serde_json::to_value(&r).unwrap();
        assert_eq!(json["session_id"], 7);
        assert_eq!(json["score"], serde_json::json!(78.0));
        assert_eq!(json["band"], "fair");
        assert_eq!(json["completeness_pct"], serde_json::json!(100.0));
        assert_eq!(json["eco2_ppm"]["avg"], serde_json::json!(600));
        assert_eq!(json["temp_f"]["avg"], serde_json::json!(76.6));
        assert_eq!(json["lowest_metric"], serde_json::json!({"metric": "temp", "avg_score": 34.0}));
    }
}
