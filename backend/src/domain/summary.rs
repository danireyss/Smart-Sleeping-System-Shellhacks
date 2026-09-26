//! Statistics over a time range of readings. Pure functions, no I/O.
//!
//! Only valid (unflagged) readings count toward the statistics. "Minutes out of
//! range" counts distinct UTC minutes with at least one valid reading outside
//! the metric's 100-point target (eCO₂ > 800 ppm, temp outside 65–70 °F,
//! RH outside 40–50%), so it is the same at 10 s and 60 s intervals.

use std::collections::HashSet;

use chrono::{DateTime, Utc};
use serde::Serialize;

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

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct MetricStats {
    pub avg: f64,
    pub min: f64,
    pub max: f64,
    pub minutes_out_of_range: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct ScoreStats {
    pub avg: f64,
    pub min: f64,
    pub max: f64,
    /// Band of the average score.
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
        eco2_ppm: metric_stats(&eco2),
        temp_f: metric_stats(&temp),
        humidity_pct: metric_stats(&humidity),
        score: min_avg_max(totals.iter().copied()).map(|(min, avg, max)| ScoreStats {
            avg,
            min,
            max,
            band: band(avg),
        }),
    }
}

fn metric_stats(samples: &[(i64, f64, f64)]) -> Option<MetricStats> {
    let (min, avg, max) = min_avg_max(samples.iter().map(|&(_, value, _)| value))?;
    let minutes_out_of_range = samples
        .iter()
        .filter(|&&(_, _, sub_score)| sub_score < 100.0)
        .map(|&(minute, _, _)| minute)
        .collect::<HashSet<_>>()
        .len();
    Some(MetricStats { avg, min, max, minutes_out_of_range })
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
        assert_eq!(temp.minutes_out_of_range, 1);
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
    fn target_edges_are_in_range() {
        let readings = [at(0, 800.0, 65.0, 40.0), at(60, 800.0, 70.0, 50.0)];
        let s = summarize(start(), start() + Duration::hours(1), &readings);
        assert_eq!(s.eco2_ppm.unwrap().minutes_out_of_range, 0);
        assert_eq!(s.temp_f.unwrap().minutes_out_of_range, 0);
        assert_eq!(s.humidity_pct.unwrap().minutes_out_of_range, 0);
    }
}
