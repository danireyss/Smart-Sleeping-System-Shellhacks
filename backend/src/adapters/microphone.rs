//! Webcam microphone sound level, via `arecord` (package `alsa-utils`).
//!
//! Records 16 kHz mono continuously, analyzes it in 125 ms windows, and every
//! minute records Leq and Lmax (dBFS plus the calibration offset = estimated
//! dB, minus the microphone's own noise if `SOUND_FLOOR_DB` is set). Audio is
//! analyzed in memory and dropped: nothing is ever stored.

use std::io::{self, Read};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use chrono::{SubsecRound, Utc};
use tokio::task::JoinHandle;
use tracing::{info, warn};

use crate::domain::ambient::{minus_noise_floor, SoundMinute, WINDOW_SAMPLES};
use crate::services::AmbientService;

/// 60 s of 125 ms windows.
const WINDOWS_PER_MINUTE: usize = 480;
const RESTART_DELAY: Duration = Duration::from_secs(10);

/// How raw dBFS becomes an estimated level in dB.
#[derive(Debug, Clone, Copy)]
pub struct Calibration {
    /// Added to dBFS (SOUND_CAL_DB).
    pub offset_db: f64,
    /// The microphone's self-noise, removed after the offset (SOUND_FLOOR_DB).
    pub floor_db: Option<f64>,
}

impl Calibration {
    fn apply(self, dbfs: f64) -> f64 {
        let db = dbfs + self.offset_db;
        self.floor_db.map_or(db, |floor| minus_noise_floor(db, floor))
    }
}

pub fn spawn(device: String, calibration: Calibration, ambient: Arc<AmbientService>) -> JoinHandle<()> {
    tokio::task::spawn_blocking(move || {
        let mut failing = false;
        loop {
            match record(&device, calibration, &ambient, &mut failing) {
                Ok(()) => warn!("microphone {device}: arecord stopped"),
                Err(e) if !failing => {
                    warn!("microphone {device}: {e}");
                    failing = true;
                }
                Err(_) => {}
            }
            thread::sleep(RESTART_DELAY);
        }
    })
}

fn record(device: &str, calibration: Calibration, ambient: &AmbientService, failing: &mut bool) -> io::Result<()> {
    let mut child: Child = Command::new("arecord")
        .args(["-q", "-D", device, "-f", "S16_LE", "-r", "16000", "-c", "1", "-t", "raw"])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;
    let mut out = child.stdout.take().ok_or_else(|| io::Error::other("no stdout"))?;

    let mut bytes = vec![0u8; WINDOW_SAMPLES * 2];
    let mut minute = SoundMinute::default();
    let result = loop {
        if let Err(e) = out.read_exact(&mut bytes) {
            break Err(e);
        }
        if *failing {
            info!("microphone {device}: recording again");
            *failing = false;
        }
        minute.add_window(&samples(&bytes));
        if minute.windows() >= WINDOWS_PER_MINUTE {
            if let Some((leq, lmax)) = minute.finish() {
                ambient.record_sound(calibration.apply(leq), calibration.apply(lmax), Utc::now().trunc_subsecs(3));
            }
            minute = SoundMinute::default();
        }
    };
    let _ = child.kill();
    let _ = child.wait();
    match result {
        Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => Ok(()),
        other => other,
    }
}

/// Little-endian 16-bit samples.
pub fn samples(bytes: &[u8]) -> Vec<i16> {
    bytes.as_chunks::<2>().0.iter().map(|&b| i16::from_le_bytes(b)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn calibration_adds_the_offset_then_removes_the_floor() {
        let plain = Calibration { offset_db: 93.0, floor_db: None };
        assert_eq!(plain.apply(-50.4), 42.6);
        let with_floor = Calibration { offset_db: 93.0, floor_db: Some(41.5) };
        assert!((with_floor.apply(-50.4) - 36.1).abs() < 0.05);
    }

    #[test]
    fn decodes_little_endian_samples() {
        assert_eq!(samples(&[0x00, 0x80, 0xff, 0x7f, 0x01, 0x00]), vec![-32768, 32767, 1]);
        assert_eq!(samples(&[0x01]), Vec::<i16>::new());
    }
}
