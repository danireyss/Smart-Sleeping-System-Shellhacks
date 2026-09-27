//! Latest light and sound values from the webcam adapters. The camera and
//! microphone record into it; ingest reads it when a sensor reading arrives and
//! attaches whatever is fresh (the MCU protocol doesn't change).

use std::sync::Mutex;

use chrono::{DateTime, Duration, Utc};

use crate::domain::ambient::Ambient;

/// Samples older than this are not attached to a reading.
pub const MAX_AGE_SECS: i64 = 120;

#[derive(Default)]
struct Latest {
    light: Option<(f64, DateTime<Utc>)>,
    /// (Leq, Lmax) in estimated dB.
    sound: Option<(f64, f64, DateTime<Utc>)>,
}

#[derive(Default)]
pub struct AmbientService {
    latest: Mutex<Latest>,
}

impl AmbientService {
    pub fn record_light(&self, level: f64, at: DateTime<Utc>) {
        if let Ok(mut l) = self.latest.lock() {
            l.light = Some((level, at));
        }
    }

    pub fn record_sound(&self, leq_db: f64, lmax_db: f64, at: DateTime<Utc>) {
        if let Ok(mut l) = self.latest.lock() {
            l.sound = Some((leq_db, lmax_db, at));
        }
    }

    /// Values recorded within `MAX_AGE_SECS` of `now`, validated.
    pub fn current(&self, now: DateTime<Utc>) -> Ambient {
        let Ok(l) = self.latest.lock() else { return Ambient::default() };
        let fresh = |at: DateTime<Utc>| now - at <= Duration::seconds(MAX_AGE_SECS);
        let light = l.light.filter(|&(_, at)| fresh(at));
        let sound = l.sound.filter(|&(_, _, at)| fresh(at));
        Ambient {
            light_level: light.map(|(v, _)| v),
            sound_db: sound.map(|(leq, _, _)| leq),
            sound_peak_db: sound.map(|(_, lmax, _)| lmax),
        }
        .validated()
    }
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::*;

    fn t(secs: i64) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 9, 26, 3, 0, 0).unwrap() + Duration::seconds(secs)
    }

    #[test]
    fn nothing_recorded_means_no_values() {
        assert_eq!(AmbientService::default().current(t(0)), Ambient::default());
    }

    #[test]
    fn fresh_values_are_attached_and_stale_ones_dropped() {
        let a = AmbientService::default();
        a.record_light(3.0, t(0));
        a.record_sound(31.0, 44.0, t(60));
        let now = a.current(t(100));
        assert_eq!(now, Ambient { light_level: Some(3.0), sound_db: Some(31.0), sound_peak_db: Some(44.0) });

        // At 130 s the light sample (from 0 s) is stale; sound (60 s) is still fresh.
        let later = a.current(t(130));
        assert_eq!(later.light_level, None);
        assert_eq!(later.sound_db, Some(31.0));
    }

    #[test]
    fn invalid_values_are_dropped() {
        let a = AmbientService::default();
        a.record_light(250.0, t(0));
        assert_eq!(a.current(t(0)).light_level, None);
    }
}
