//! A single sensor reading and the checks that decide whether it can be scored.
//! Pure types, no I/O.

use chrono::{DateTime, Utc};
use serde::{Serialize, Serializer};

use super::ambient::Ambient;
use super::round::{self, round1};

/// CCS811 needs ~20 minutes after power-on before eCO₂ is meaningful.
pub const WARM_UP_SECS: u64 = 1200;

pub const ECO2_RANGE_PPM: (f64, f64) = (400.0, 8192.0);
pub const HUMIDITY_RANGE_PCT: (f64, f64) = (0.0, 100.0);
pub const TEMP_RANGE_F: (f64, f64) = (32.0, 120.0);

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Reading {
    /// When the backend received the reading (UTC). The MCU has no clock.
    pub received_at: DateTime<Utc>,
    /// Estimated CO₂ from the CCS811 (eCO₂), ppm.
    #[serde(serialize_with = "round::whole")]
    pub eco2_ppm: f64,
    #[serde(serialize_with = "round::whole")]
    pub tvoc_ppb: f64,
    /// `None` when the DHT11 read failed (the MCU sends NaN).
    #[serde(serialize_with = "round::tenths_opt")]
    pub temp_f: Option<f64>,
    /// `None` when the DHT11 read failed (the MCU sends NaN).
    #[serde(serialize_with = "round::tenths_opt")]
    pub humidity_pct: Option<f64>,
    /// Seconds since the sketch started, used for CCS811 warm-up.
    pub uptime_s: u64,
    /// Light and sound from the webcam, if a fresh sample was available.
    #[serde(flatten)]
    pub ambient: Ambient,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Flag {
    WarmUp,
    Eco2Zero,
    Eco2OutOfRange,
    TempMissing,
    TempOutOfRange,
    HumidityMissing,
    HumidityOutOfRange,
}

impl Flag {
    /// Stable name, used in storage and logs.
    pub fn as_str(self) -> &'static str {
        match self {
            Flag::WarmUp => "warm_up",
            Flag::Eco2Zero => "eco2_zero",
            Flag::Eco2OutOfRange => "eco2_out_of_range",
            Flag::TempMissing => "temp_missing",
            Flag::TempOutOfRange => "temp_out_of_range",
            Flag::HumidityMissing => "humidity_missing",
            Flag::HumidityOutOfRange => "humidity_out_of_range",
        }
    }
}

impl Serialize for Flag {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl Reading {
    /// The reading at API precision: eCO₂ and TVOC whole, temperature and humidity
    /// to 1 decimal. Flags, scores, and statistics use these values, so they always
    /// match the numbers shown (79.0 °F scores exactly 10.0, not 10.2 from 78.98).
    pub fn rounded(&self) -> Reading {
        Reading {
            eco2_ppm: self.eco2_ppm.round(),
            tvoc_ppb: self.tvoc_ppb.round(),
            temp_f: self.temp_f.map(round1),
            humidity_pct: self.humidity_pct.map(round1),
            ambient: Ambient {
                light_level: self.ambient.light_level.map(round1),
                sound_db: self.ambient.sound_db.map(round1),
                sound_peak_db: self.ambient.sound_peak_db.map(round1),
            },
            ..self.clone()
        }
    }

    /// Reasons this reading should be excluded from scoring, judged on the rounded
    /// values. Empty means valid.
    pub fn flags(&self) -> Vec<Flag> {
        let r = self.rounded();
        let mut flags = Vec::new();
        if r.uptime_s < WARM_UP_SECS {
            flags.push(Flag::WarmUp);
        }
        if r.eco2_ppm == 0.0 {
            flags.push(Flag::Eco2Zero);
        } else if !in_range(r.eco2_ppm, ECO2_RANGE_PPM) {
            flags.push(Flag::Eco2OutOfRange);
        }
        match r.temp_f {
            None => flags.push(Flag::TempMissing),
            Some(t) if !in_range(t, TEMP_RANGE_F) => flags.push(Flag::TempOutOfRange),
            Some(_) => {}
        }
        match r.humidity_pct {
            None => flags.push(Flag::HumidityMissing),
            Some(h) if !in_range(h, HUMIDITY_RANGE_PCT) => flags.push(Flag::HumidityOutOfRange),
            Some(_) => {}
        }
        flags
    }
}

fn in_range(v: f64, (lo, hi): (f64, f64)) -> bool {
    (lo..=hi).contains(&v)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reading() -> Reading {
        Reading {
            received_at: Utc::now(),
            eco2_ppm: 450.0,
            tvoc_ppb: 5.0,
            temp_f: Some(68.0),
            humidity_pct: Some(45.0),
            uptime_s: WARM_UP_SECS,
            ambient: Ambient::default(),
        }
    }

    #[test]
    fn valid_reading_has_no_flags() {
        assert!(reading().flags().is_empty());
    }

    #[test]
    fn warm_up_ends_at_1200_seconds() {
        let r = Reading { uptime_s: WARM_UP_SECS - 1, ..reading() };
        assert_eq!(r.flags(), vec![Flag::WarmUp]);
    }

    #[test]
    fn zero_eco2_is_flagged_once() {
        let r = Reading { eco2_ppm: 0.0, ..reading() };
        assert_eq!(r.flags(), vec![Flag::Eco2Zero]);
    }

    #[test]
    fn eco2_range_is_inclusive() {
        for ok in [400.0, 8192.0] {
            assert!(Reading { eco2_ppm: ok, ..reading() }.flags().is_empty());
        }
        for bad in [399.0, 8193.0] {
            let r = Reading { eco2_ppm: bad, ..reading() };
            assert_eq!(r.flags(), vec![Flag::Eco2OutOfRange]);
        }
    }

    #[test]
    fn ranges_are_judged_on_rounded_values() {
        // 120.04 °F is shown as 120.0, which is in range.
        assert!(Reading { temp_f: Some(120.04), ..reading() }.flags().is_empty());
        let r = Reading { temp_f: Some(120.06), ..reading() }; // shown as 120.1
        assert_eq!(r.flags(), vec![Flag::TempOutOfRange]);
        assert!(Reading { humidity_pct: Some(100.04), ..reading() }.flags().is_empty());
        assert!(Reading { eco2_ppm: 399.6, ..reading() }.flags().is_empty()); // shown as 400
        assert_eq!(Reading { eco2_ppm: 0.4, ..reading() }.flags(), vec![Flag::Eco2Zero]);
    }

    #[test]
    fn rounded_matches_api_precision() {
        let r = Reading {
            eco2_ppm: 477.4,
            tvoc_ppb: 10.6,
            temp_f: Some(78.98),
            humidity_pct: Some(49.7999992370605),
            ..reading()
        }
        .rounded();
        assert_eq!((r.eco2_ppm, r.tvoc_ppb), (477.0, 11.0));
        assert_eq!((r.temp_f, r.humidity_pct), (Some(79.0), Some(49.8)));
        assert_eq!(Reading { temp_f: None, ..reading() }.rounded().temp_f, None);
    }

    #[test]
    fn missing_dht_values_are_flagged() {
        let r = Reading { temp_f: None, humidity_pct: None, ..reading() };
        assert_eq!(r.flags(), vec![Flag::TempMissing, Flag::HumidityMissing]);
    }

    #[test]
    fn out_of_range_dht_values_are_flagged() {
        let r = Reading { temp_f: Some(121.0), humidity_pct: Some(-1.0), ..reading() };
        assert_eq!(r.flags(), vec![Flag::TempOutOfRange, Flag::HumidityOutOfRange]);
        let r = Reading { temp_f: Some(31.9), humidity_pct: Some(100.1), ..reading() };
        assert_eq!(r.flags(), vec![Flag::TempOutOfRange, Flag::HumidityOutOfRange]);
    }
}
