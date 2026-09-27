//! Light and sound from the webcam: the values attached to readings, and the
//! math that turns a frame or audio samples into them. Pure, no I/O.
//!
//! Privacy: frames and audio are reduced to these numbers in memory and then
//! dropped. Only the numbers are stored, served, or given to the agent.
//!
//! - `light_level`: mean brightness of a small grayscale frame with the camera's
//!   exposure locked, 0 (black) to 100 (white). Relative, not lux: "estimated".
//! - `sound_db`: Leq (energy average) over the minute; `sound_peak_db`: Lmax (the
//!   loudest 125 ms window). dBFS from the mic plus a calibration offset, so an
//!   estimate of dB SPL (not A-weighted): "estimated".

use serde::Serialize;

use super::round;

/// Light and sound attached to one reading; `None` when there was no fresh
/// sample (no camera, device error, or a value outside the valid range).
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize)]
pub struct Ambient {
    #[serde(serialize_with = "round::tenths_opt")]
    pub light_level: Option<f64>,
    #[serde(serialize_with = "round::tenths_opt")]
    pub sound_db: Option<f64>,
    #[serde(serialize_with = "round::tenths_opt")]
    pub sound_peak_db: Option<f64>,
}

pub const LIGHT_RANGE: (f64, f64) = (0.0, 100.0);
pub const SOUND_RANGE_DB: (f64, f64) = (0.0, 130.0);

impl Ambient {
    /// Drops values outside the valid ranges (a bad value removes only that metric).
    pub fn validated(self) -> Ambient {
        let within = |v: Option<f64>, (lo, hi): (f64, f64)| v.filter(|v| v.is_finite() && (lo..=hi).contains(v));
        Ambient {
            light_level: within(self.light_level, LIGHT_RANGE),
            sound_db: within(self.sound_db, SOUND_RANGE_DB),
            sound_peak_db: within(self.sound_peak_db, SOUND_RANGE_DB),
        }
    }
}

/// Mean brightness of 8-bit grayscale pixels, scaled to 0–100.
pub fn light_level(gray: &[u8]) -> Option<f64> {
    if gray.is_empty() {
        return None;
    }
    let sum: u64 = gray.iter().map(|&p| p as u64).sum();
    Some(sum as f64 / gray.len() as f64 / 255.0 * 100.0)
}

/// Samples per analysis window: 125 ms at 16 kHz (the "fast" sound-meter time).
pub const WINDOW_SAMPLES: usize = 2000;
const SILENCE_DBFS: f64 = -120.0;

/// Mean square of 16-bit samples, normalized to full scale (1.0 = full-scale square wave).
pub fn mean_square(samples: &[i16]) -> f64 {
    if samples.is_empty() {
        return 0.0;
    }
    let sum: f64 = samples.iter().map(|&s| (s as f64 / 32768.0).powi(2)).sum();
    sum / samples.len() as f64
}

/// Power (mean square) to dBFS.
pub fn to_dbfs(power: f64) -> f64 {
    if power <= 0.0 {
        SILENCE_DBFS
    } else {
        (10.0 * power.log10()).max(SILENCE_DBFS)
    }
}

/// Accumulates 125 ms windows over a minute into Leq and Lmax (dBFS).
#[derive(Debug, Default)]
pub struct SoundMinute {
    energy_sum: f64,
    windows: usize,
    peak_power: f64,
}

impl SoundMinute {
    pub fn add_window(&mut self, samples: &[i16]) {
        let p = mean_square(samples);
        self.energy_sum += p;
        self.windows += 1;
        self.peak_power = self.peak_power.max(p);
    }

    pub fn windows(&self) -> usize {
        self.windows
    }

    /// (Leq, Lmax) in dBFS, or `None` if no audio arrived.
    pub fn finish(&self) -> Option<(f64, f64)> {
        (self.windows > 0).then(|| (to_dbfs(self.energy_sum / self.windows as f64), to_dbfs(self.peak_power)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-6
    }

    #[test]
    fn light_level_is_mean_brightness_0_to_100() {
        assert_eq!(light_level(&[]), None);
        assert!(close(light_level(&[0; 768]).unwrap(), 0.0));
        assert!(close(light_level(&[255; 768]).unwrap(), 100.0));
        assert!(close(light_level(&[0, 255]).unwrap(), 50.0));
        assert!(close(light_level(&[51; 10]).unwrap(), 20.0));
    }

    #[test]
    fn dbfs_of_known_signals() {
        // Full-scale square wave: power 1.0 -> 0 dBFS.
        let square: Vec<i16> = (0..WINDOW_SAMPLES).map(|i| if i % 2 == 0 { 32767 } else { -32768 }).collect();
        assert!(to_dbfs(mean_square(&square)) > -0.01);
        // Half amplitude: -6.02 dBFS.
        let half: Vec<i16> = (0..WINDOW_SAMPLES).map(|i| if i % 2 == 0 { 16384 } else { -16384 }).collect();
        assert!((to_dbfs(mean_square(&half)) + 6.02).abs() < 0.01);
        assert_eq!(to_dbfs(mean_square(&[0; 100])), -120.0);
    }

    #[test]
    fn leq_is_an_energy_average_and_lmax_the_loudest_window() {
        let loud = vec![16384i16; WINDOW_SAMPLES]; // power 0.25 -> -6.02 dBFS
        let quiet = vec![0i16; WINDOW_SAMPLES];
        let mut minute = SoundMinute::default();
        assert_eq!(minute.finish(), None);
        minute.add_window(&loud);
        for _ in 0..9 {
            minute.add_window(&quiet);
        }
        let (leq, lmax) = minute.finish().unwrap();
        // One loud window in ten: energy average is 10 dB below it.
        assert!((leq - (-6.02 - 10.0)).abs() < 0.01, "{leq}");
        assert!((lmax + 6.02).abs() < 0.01, "{lmax}");
        assert_eq!(minute.windows(), 10);
    }

    #[test]
    fn validated_drops_only_the_bad_metric() {
        let a = Ambient { light_level: Some(120.0), sound_db: Some(35.0), sound_peak_db: Some(f64::NAN) };
        assert_eq!(a.validated(), Ambient { light_level: None, sound_db: Some(35.0), sound_peak_db: None });
    }
}
