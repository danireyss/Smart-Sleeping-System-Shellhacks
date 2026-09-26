//! Statistics over a time range of readings. Pure functions, no I/O.
//!
//! Only valid (unflagged) readings count toward the statistics, which use the
//! values at API precision (`Reading::rounded`), like the scores. "Minutes out of
//! range" counts distinct UTC minutes with at least one valid reading outside
//! the metric's 100-point target (eCO₂ > 800 ppm, temp outside 65–70 °F,
//! RH outside 40–50%), so it is the same at 10 s and 60 s intervals.
//!
//! Serialized with API rounding: eCO₂ stats as whole numbers, temperature,
//! humidity, and scores to 1 decimal. The score band uses the rounded average.

use std::collections::HashSet;

use chrono::{DateTime, Utc};
use serde::ser::SerializeStruct;
use serde::{Serialize, Serializer};

use super::round::{self, round1, Precision, Rounded};
use super::scoring::{self, band, Band};
use super::Reading;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Summary {
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
    /// All readings in the range, including flagged ones.
    pub readings: usize,
    pub valid_readings: usize,
    /// Distinct minutes with at least one valid reading.
    pub valid_minutes: usize,
    /// `None` when there are no valid readings.
    pub eco2_ppm: Option<MetricStats>,
    pub temp_f: Option<MetricStats>,
    pub humidity_pct: Option<MetricStats>,
    pub score: Option<ScoreStats>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MetricStats {
    pub avg: f64,
    pub min: f64,
    pub max: f64,
    /// Average of this metric's 0–100 sub-score.
    pub avg_score: f64,
    pub minutes_out_of_range: usize,
    /// How `avg`/`min`/`max` are serialized (eCO₂ whole, others 1 decimal).
    pub precision: Precision,
}

impl Serialize for MetricStats {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let mut st = s.serialize_struct("MetricStats", 5)?;
        st.serialize_field("avg", &Rounded(self.avg, self.precision))?;
        st.serialize_field("min", &Rounded(self.min, self.precision))?;
        st.serialize_field("max", &Rounded(self.max, self.precision))?;
        st.serialize_field("avg_score", &Rounded(self.avg_score, Precision::Tenths))?;
        st.serialize_field("minutes_out_of_range", &self.minutes_out_of_range)?;
        st.end()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct ScoreStats {
    #[serde(serialize_with = "round::tenths")]
    pub avg: f64,
    #[serde(serialize_with = "round::tenths")]
    pub min: f64,
    #[serde(serialize_with = "round::tenths")]
    pub max: f64,
    /// Band of the average score, rounded to 1 decimal.
    pub band: Band,
}

/// Summarizes `readings`, which should all fall in `[start, end)`.
pub fn summarize(start: DateTime<Utc>, end: DateTime<Utc>, readings: &[Reading]) -> Summary {
    // (minute, value, sub-score) per valid reading, per metric.
    let mut eco2 = Vec::new();
    let mut temp = Vec::new();
    let mut humidity = Vec::new();
    let mut totals = Vec::new();
    let mut minutes = HashSet::new();

    for r in readings {
        let Some(s) = scoring::score(r) else { continue };
        let r = r.rounded();
        let (Some(t), Some(h)) = (r.temp_f, r.humidity_pct) else { continue };
        let minute = r.received_at.timestamp().div_euclid(60);
        minutes.insert(minute);
        eco2.push((minute, r.eco2_ppm, s.eco2));
        temp.push((minute, t, s.temp));
        humidity.push((minute, h, s.humidity));
        totals.push(s.total);
    }

    Summary {
        start,
        end,
        readings: readings.len(),
        valid_readings: totals.len(),
        valid_minutes: minutes.len(),
        eco2_ppm: metric_stats(&eco2, Precision::Whole),
        temp_f: metric_stats(&temp, Precision::Tenths),
        humidity_pct: metric_stats(&humidity, Precision::Tenths),
        score: min_avg_max(totals.iter().copied()).map(|(min, avg, max)| ScoreStats {
            avg,
            min,
            max,
            band: band(round1(avg)),
        }),
    }
}

fn metric_stats(samples: &[(i64, f64, f64)], precision: Precision) -> Option<MetricStats> {
    let (min, avg, max) = min_avg_max(samples.iter().map(|&(_, value, _)| value))?;
    let (_, avg_score, _) = min_avg_max(samples.iter().map(|&(_, _, sub_score)| sub_score))?;
    let minutes_out_of_range = samples
        .iter()
        .filter(|&&(_, _, sub_score)| sub_score < 100.0)
        .map(|&(minute, _, _)| minute)
        .collect::<HashSet<_>>()
        .len();
    Some(MetricStats { avg, min, max, avg_score, minutes_out_of_range, precision })
}

fn min_avg_max(values: impl Iterator<Item = f64>) -> Option<(f64, f64, f64)> {
    let (mut min, mut max, mut sum, mut n) = (f64::INFINITY, f64::NEG_INFINITY, 0.0, 0usize);
    for v in values {
        min = min.min(v);
        max = max.max(v);
        sum += v;
        n += 1;
    }
    (n > 0).then(|| (min, sum / n as f64, max))
}

#[cfg(test)]
mod tests {
    use chrono::{Duration, TimeZone};

    use super::*;
    use crate::domain::reading::WARM_UP_SECS;

    fn start() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 9, 26, 6, 0, 0).unwrap()
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

    fn assert_close(actual: f64, expected: f64) {
        assert!((actual - expected).abs() < 1e-9, "expected {expected}, got {actual}");
    }

    #[test]
    fn empty_range_has_no_stats() {
        let s = summarize(start(), start() + Duration::hours(1), &[]);
        assert_eq!((s.readings, s.valid_readings, s.valid_minutes), (0, 0, 0));
        assert!(s.eco2_ppm.is_none() && s.temp_f.is_none() && s.humidity_pct.is_none());
        assert!(s.score.is_none());
    }

    #[test]
    fn stats_over_valid_readings() {
        let readings = [
            at(0, 600.0, 68.0, 45.0),   // all in target, score 100
            at(60, 1400.0, 72.5, 45.0), // eCO2 50, temp 75, RH 100 -> 75
        ];
        let s = summarize(start(), start() + Duration::hours(1), &readings);
        assert_eq!((s.readings, s.valid_readings, s.valid_minutes), (2, 2, 2));

        let eco2 = s.eco2_ppm.unwrap();
        assert_close(eco2.min, 600.0);
        assert_close(eco2.avg, 1000.0);
        assert_close(eco2.max, 1400.0);
        assert_eq!(eco2.minutes_out_of_range, 1);

        let temp = s.temp_f.unwrap();
        assert_close(temp.avg, 70.25);
        assert_close(temp.avg_score, 87.5);
        assert_eq!(temp.minutes_out_of_range, 1);
        assert_close(eco2.avg_score, 75.0);
        assert_eq!(s.humidity_pct.unwrap().minutes_out_of_range, 0);

        let score = s.score.unwrap();
        assert_close(score.min, 75.0);
        assert_close(score.avg, 87.5);
        assert_close(score.max, 100.0);
        assert_eq!(score.band, Band::Good);
    }

    #[test]
    fn flagged_readings_are_counted_but_excluded_from_stats() {
        let warming = Reading { uptime_s: 10, ..at(0, 5000.0, 90.0, 90.0) };
        let missing = Reading { temp_f: None, ..at(10, 600.0, 68.0, 45.0) };
        let valid = at(20, 600.0, 68.0, 45.0);
        let s = summarize(start(), start() + Duration::hours(1), &[warming, missing, valid]);
        assert_eq!((s.readings, s.valid_readings, s.valid_minutes), (3, 1, 1));
        assert_close(s.eco2_ppm.unwrap().max, 600.0);
        assert_eq!(s.temp_f.unwrap().minutes_out_of_range, 0);
    }

    #[test]
    fn out_of_range_counts_distinct_minutes() {
        // Six readings 10 s apart in one minute, all too warm, plus one in the next minute.
        let mut readings: Vec<_> = (0..6).map(|i| at(i * 10, 600.0, 77.0, 45.0)).collect();
        readings.push(at(60, 600.0, 77.0, 45.0));
        let s = summarize(start(), start() + Duration::hours(1), &readings);
        assert_eq!(s.valid_readings, 7);
        assert_eq!(s.valid_minutes, 2);
        assert_eq!(s.temp_f.unwrap().minutes_out_of_range, 2);
        assert_eq!(s.eco2_ppm.unwrap().minutes_out_of_range, 0);
    }

    #[test]
    fn serializes_with_api_rounding() {
        // Shown values 477 / 76.6 and 490 / 77.2. Totals: (100 + 34 + 100) / 3 = 78.0
        // and (100 + 28 + 100) / 3 = 76.0.
        let readings = [at(0, 477.4, 76.64, 49.8), at(60, 490.0, 77.2, 50.0)];
        let s = summarize(start(), start() + Duration::hours(1), &readings);
        let json = serde_json::to_value(&s).unwrap();
        assert_eq!(json["eco2_ppm"]["avg"], serde_json::json!(484)); // 483.5 -> 484
        assert!(json["eco2_ppm"]["min"].is_i64());
        assert_eq!(json["temp_f"]["avg"], serde_json::json!(76.9));
        assert_eq!(json["humidity_pct"]["avg"], serde_json::json!(49.9));
        assert_eq!(json["temp_f"]["avg_score"], serde_json::json!(31.0));
        assert_eq!(json["score"]["avg"], serde_json::json!(77.0));
    }

    #[test]
    fn out_of_range_uses_shown_values() {
        // 70.04 °F is shown as 70.0: on target. 70.06 is shown as 70.1: out of range.
        let readings = [at(0, 600.0, 70.04, 45.0), at(60, 600.0, 70.06, 45.0)];
        let s = summarize(start(), start() + Duration::hours(1), &readings);
        let temp = s.temp_f.unwrap();
        assert_eq!(temp.minutes_out_of_range, 1);
        assert_eq!((temp.min, temp.max), (70.0, 70.1));
    }

    #[test]
    fn target_edges_are_in_range() {
        let readings = [at(0, 800.0, 65.0, 40.0), at(60, 800.0, 70.0, 50.0)];
        let s = summarize(start(), start() + Duration::hours(1), &readings);
        assert_eq!(s.eco2_ppm.unwrap().minutes_out_of_range, 0);
        assert_eq!(s.temp_f.unwrap().minutes_out_of_range, 0);
        assert_eq!(s.humidity_pct.unwrap().minutes_out_of_range, 0);
    }
}
