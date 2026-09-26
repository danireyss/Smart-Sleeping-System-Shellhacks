//! Sleep-environment scoring (see "Scoring" in CLAUDE.md). Pure functions, no I/O.
//!
//! | Metric      | 100 points  | Outside            | 0 points at        |
//! | ----------- | ----------- | ------------------ | ------------------ |
//! | eCO₂        | ≤ 800 ppm   | linear             | ≥ 2,000 ppm        |
//! | Temperature | 65–70 °F    | −10 per °F outside | ≤ 55 or ≥ 80 °F    |
//! | Humidity    | 40–60% RH   | −5 per % outside   | ≤ 20% or ≥ 80%     |
//!
//! Sources for these targets: docs/REFERENCES.md.
//!
//! Minute score = average of the three sub-scores. Readings arrive once a minute
//! in production, so a reading's score is its minute's score.
//!
//! Scores are computed from the reading at API precision (`Reading::rounded`), so
//! a shown 79.0 °F always scores 10.0.
//!
//! Reported scores are rounded to 1 decimal: each sub-score is rounded, the total
//! is the rounded average of the rounded sub-scores, and the band comes from that
//! rounded total, so the band always matches the number shown.

use serde::Serialize;

use super::round::{self, round1};
use super::{Flag, Reading};

pub const ECO2_FULL_PPM: f64 = 800.0;
pub const ECO2_ZERO_PPM: f64 = 2000.0;

pub const TEMP_TARGET_F: (f64, f64) = (65.0, 70.0);
pub const TEMP_POINTS_PER_F: f64 = 10.0;

pub const HUMIDITY_TARGET_PCT: (f64, f64) = (40.0, 60.0);
pub const HUMIDITY_POINTS_PER_PCT: f64 = 5.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Band {
    Great,
    Good,
    Fair,
    Poor,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct MinuteScore {
    #[serde(serialize_with = "round::tenths")]
    pub eco2: f64,
    #[serde(serialize_with = "round::tenths")]
    pub temp: f64,
    #[serde(serialize_with = "round::tenths")]
    pub humidity: f64,
    #[serde(serialize_with = "round::tenths")]
    pub total: f64,
    pub band: Band,
}

/// A reading with its flags and score (`None` when flagged). This is what the
/// API returns and what the live stream sends.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ScoredReading {
    #[serde(flatten)]
    pub reading: Reading,
    pub flags: Vec<Flag>,
    pub score: Option<MinuteScore>,
}

impl From<Reading> for ScoredReading {
    fn from(reading: Reading) -> Self {
        let flags = reading.flags();
        let score = score(&reading);
        Self { reading, flags, score }
    }
}

/// 100 at ≤ 800 ppm, 0 at ≥ 2,000 ppm, linear in between.
pub fn eco2_score(ppm: f64) -> f64 {
    let fraction = (ECO2_ZERO_PPM - ppm) / (ECO2_ZERO_PPM - ECO2_FULL_PPM);
    (fraction * 100.0).clamp(0.0, 100.0)
}

/// 100 at 65–70 °F, minus 10 per °F outside (0 at ≤ 55 or ≥ 80 °F).
pub fn temp_score(temp_f: f64) -> f64 {
    target_score(temp_f, TEMP_TARGET_F, TEMP_POINTS_PER_F)
}

/// 100 at 40–60% RH, minus 5 per % outside (0 at ≤ 20% or ≥ 80%).
pub fn humidity_score(humidity_pct: f64) -> f64 {
    target_score(humidity_pct, HUMIDITY_TARGET_PCT, HUMIDITY_POINTS_PER_PCT)
}

/// Great 90–100, Good 80–89, Fair 70–79, Poor < 70.
/// Fractional scores band by their floor: 89.9 is Good.
pub fn band(score: f64) -> Band {
    if score >= 90.0 {
        Band::Great
    } else if score >= 80.0 {
        Band::Good
    } else if score >= 70.0 {
        Band::Fair
    } else {
        Band::Poor
    }
}

/// Score for one reading, or `None` if the reading is flagged (excluded from scoring).
/// Uses the rounded values, so the score matches the numbers shown.
pub fn score(reading: &Reading) -> Option<MinuteScore> {
    if !reading.flags().is_empty() {
        return None;
    }
    let reading = reading.rounded();
    // Unflagged readings always have temperature and humidity.
    let (Some(temp_f), Some(humidity_pct)) = (reading.temp_f, reading.humidity_pct) else {
        return None;
    };
    let eco2 = round1(eco2_score(reading.eco2_ppm));
    let temp = round1(temp_score(temp_f));
    let humidity = round1(humidity_score(humidity_pct));
    let total = round1((eco2 + temp + humidity) / 3.0);
    Some(MinuteScore { eco2, temp, humidity, total, band: band(total) })
}

fn target_score(value: f64, (lo, hi): (f64, f64), points_per_unit: f64) -> f64 {
    let distance = if value < lo {
        lo - value
    } else if value > hi {
        value - hi
    } else {
        0.0
    };
    (100.0 - distance * points_per_unit).clamp(0.0, 100.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::reading::WARM_UP_SECS;
    use chrono::Utc;

    fn assert_close(actual: f64, expected: f64) {
        assert!((actual - expected).abs() < 1e-9, "expected {expected}, got {actual}");
    }

    fn reading(eco2: f64, temp: f64, humidity: f64) -> Reading {
        Reading {
            received_at: Utc::now(),
            eco2_ppm: eco2,
            tvoc_ppb: 0.0,
            temp_f: Some(temp),
            humidity_pct: Some(humidity),
            uptime_s: WARM_UP_SECS,
        }
    }

    #[test]
    fn eco2_edges_and_slope() {
        assert_close(eco2_score(400.0), 100.0);
        assert_close(eco2_score(800.0), 100.0);
        assert_close(eco2_score(1400.0), 50.0);
        assert_close(eco2_score(1100.0), 75.0);
        assert_close(eco2_score(2000.0), 0.0);
        assert_close(eco2_score(5000.0), 0.0);
    }

    #[test]
    fn temp_edges_and_slope() {
        assert_close(temp_score(65.0), 100.0);
        assert_close(temp_score(67.5), 100.0);
        assert_close(temp_score(70.0), 100.0);
        assert_close(temp_score(64.0), 90.0);
        assert_close(temp_score(72.5), 75.0);
        assert_close(temp_score(77.7), 23.0); // what the board's DHT11 reads
        assert_close(temp_score(55.0), 0.0);
        assert_close(temp_score(80.0), 0.0);
        assert_close(temp_score(50.0), 0.0);
        assert_close(temp_score(85.0), 0.0);
    }

    #[test]
    fn humidity_edges_and_slope() {
        assert_close(humidity_score(40.0), 100.0);
        assert_close(humidity_score(51.9), 100.0); // last night's average
        assert_close(humidity_score(60.0), 100.0);
        assert_close(humidity_score(39.0), 95.0);
        assert_close(humidity_score(65.0), 75.0);
        assert_close(humidity_score(70.0), 50.0);
        assert_close(humidity_score(20.0), 0.0);
        assert_close(humidity_score(80.0), 0.0);
        assert_close(humidity_score(10.0), 0.0);
        assert_close(humidity_score(90.0), 0.0);
    }

    #[test]
    fn band_edges() {
        assert_eq!(band(100.0), Band::Great);
        assert_eq!(band(90.0), Band::Great);
        assert_eq!(band(89.9), Band::Good);
        assert_eq!(band(80.0), Band::Good);
        assert_eq!(band(79.9), Band::Fair);
        assert_eq!(band(70.0), Band::Fair);
        assert_eq!(band(69.9), Band::Poor);
        assert_eq!(band(0.0), Band::Poor);
    }

    #[test]
    fn minute_score_averages_sub_scores() {
        let s = score(&reading(1400.0, 72.5, 45.0)).unwrap();
        assert_close(s.eco2, 50.0);
        assert_close(s.temp, 75.0);
        assert_close(s.humidity, 100.0);
        assert_close(s.total, 75.0);
        assert_eq!(s.band, Band::Fair);

        // Board conditions: (100 + 23 + 100) / 3 ≈ 74.3
        let board = score(&reading(410.0, 77.7, 49.0)).unwrap();
        assert_close(board.temp, 23.0);
        assert_close(board.total, 74.3);
        assert_eq!(board.band, Band::Fair);

        // 76.64 °F is shown as 76.6: temp 34.0, total (100 + 34 + 100) / 3 = 78.0
        let s = score(&reading(477.0, 76.64, 49.8)).unwrap();
        assert_eq!(s.temp, 34.0);
        assert_eq!(s.total, 78.0);

        let perfect = score(&reading(600.0, 68.0, 45.0)).unwrap();
        assert_close(perfect.total, 100.0);
        assert_eq!(perfect.band, Band::Great);
    }

    #[test]
    fn scores_the_shown_values() {
        // 78.98 °F is shown as 79.0, so the temp sub-score is 10.0 (not 10.2).
        let s = score(&reading(400.0, 78.98, 48.6)).unwrap();
        assert_eq!(s.temp, 10.0);
        assert_eq!(s.total, 70.0); // (100 + 10 + 100) / 3
        // 800.4 ppm is shown as 800, which is on target.
        assert_eq!(score(&reading(800.4, 68.0, 45.0)).unwrap().eco2, 100.0);
        // 50.04% RH is shown as 50.0, which is on target.
        assert_eq!(score(&reading(600.0, 68.0, 50.04)).unwrap().humidity, 100.0);
    }

    #[test]
    fn band_comes_from_the_rounded_total() {
        // 1161 ppm -> eCO2 69.9. Unrounded total (69.9 + 100 + 100) / 3 = 89.97
        // would be Good, but it is shown as 90.0, so the band must be Great.
        let s = score(&reading(1161.0, 68.0, 45.0)).unwrap();
        assert_eq!(s.eco2, 69.9);
        assert_eq!(s.total, 90.0);
        assert_eq!(s.band, Band::Great);
    }

    #[test]
    fn flagged_readings_are_not_scored() {
        let ok = reading(600.0, 68.0, 45.0);
        assert!(score(&ok).is_some());

        let flagged = [
            Reading { uptime_s: WARM_UP_SECS - 1, ..ok.clone() },
            Reading { eco2_ppm: 0.0, ..ok.clone() },
            Reading { eco2_ppm: 9000.0, ..ok.clone() },
            Reading { temp_f: None, ..ok.clone() },
            Reading { temp_f: Some(130.0), ..ok.clone() },
            Reading { humidity_pct: None, ..ok.clone() },
            Reading { humidity_pct: Some(101.0), ..ok.clone() },
        ];
        for r in &flagged {
            assert!(score(r).is_none(), "should be excluded: {r:?}");
        }
    }
}
