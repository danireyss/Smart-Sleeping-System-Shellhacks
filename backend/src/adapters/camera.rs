//! Webcam light level, via `v4l2-ctl` and `ffmpeg` (packages `v4l-utils` and
//! `ffmpeg` on the board).
//!
//! At start (and after errors) the camera's automatic exposure is turned off and
//! the exposure fixed, so brightness follows the room's light instead of being
//! evened out by the camera. The LED is turned off if the driver allows it.
//! Every minute one frame is grabbed as a 32x24 grayscale image, reduced to a
//! single 0–100 brightness value, and dropped: no image is ever stored.

use std::io::Read;
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use chrono::{SubsecRound, Utc};
use tokio::task::JoinHandle;
use tracing::{info, warn};

use crate::domain::ambient::light_level;
use crate::services::AmbientService;

const INTERVAL: Duration = Duration::from_secs(60);
const CAPTURE_TIMEOUT: Duration = Duration::from_secs(20);
const FRAME_W: usize = 32;
const FRAME_H: usize = 24;

pub fn spawn(device: String, exposure: i64, ambient: Arc<AmbientService>) -> JoinHandle<()> {
    tokio::task::spawn_blocking(move || {
        let mut locked = false;
        let mut failing = false;
        loop {
            if !locked {
                match lock_exposure(&device, exposure) {
                    Ok(set) => {
                        info!("camera {device}: set {}", set.join(", "));
                        locked = true;
                    }
                    Err(e) => warn!("camera {device}: could not set controls: {e}"),
                }
            }
            match capture_gray(&device).and_then(|f| light_level(&f).ok_or_else(|| "empty frame".into())) {
                Ok(level) => {
                    if failing {
                        info!("camera {device}: capturing again");
                        failing = false;
                    }
                    ambient.record_light(level, Utc::now().trunc_subsecs(3));
                }
                Err(e) => {
                    // Log once per outage, not every minute.
                    if !failing {
                        warn!("camera {device}: {e}");
                        failing = true;
                    }
                    locked = false; // the camera may have been unplugged; set controls again
                }
            }
            thread::sleep(INTERVAL);
        }
    })
}

/// Turns off auto exposure, fixes the exposure time, and turns off the LED,
/// using whichever control names this driver has. Returns what was set.
fn lock_exposure(device: &str, exposure: i64) -> Result<Vec<String>, String> {
    let out = Command::new("v4l2-ctl")
        .args(["-d", device, "--list-ctrls"])
        .output()
        .map_err(|e| format!("v4l2-ctl: {e}"))?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    let settings = exposure_settings(&control_names(&String::from_utf8_lossy(&out.stdout)), exposure);
    if settings.is_empty() {
        return Err("no exposure controls found".into());
    }
    let status = Command::new("v4l2-ctl")
        .args(["-d", device, "--set-ctrl", &settings.join(",")])
        .status()
        .map_err(|e| format!("v4l2-ctl: {e}"))?;
    if !status.success() {
        return Err(format!("v4l2-ctl --set-ctrl {} failed", settings.join(",")));
    }
    Ok(settings)
}

/// Control names from `v4l2-ctl --list-ctrls` output (first word of each control line).
pub fn control_names(list_ctrls: &str) -> Vec<String> {
    list_ctrls
        .lines()
        .filter(|l| l.contains(" 0x"))
        .filter_map(|l| l.split_whitespace().next())
        .map(str::to_string)
        .collect()
}

/// The `name=value` settings to lock exposure, for the controls this camera has.
/// Newer kernels call them `auto_exposure`/`exposure_time_absolute`; older ones
/// `exposure_auto`/`exposure_absolute`. Mode 1 is "manual" in both.
pub fn exposure_settings(names: &[String], exposure: i64) -> Vec<String> {
    let has = |n: &str| names.iter().any(|x| x == n);
    let mut set = Vec::new();
    for mode in ["auto_exposure", "exposure_auto"] {
        if has(mode) {
            set.push(format!("{mode}=1"));
        }
    }
    for time in ["exposure_time_absolute", "exposure_absolute"] {
        if has(time) {
            set.push(format!("{time}={exposure}"));
        }
    }
    for off in ["exposure_dynamic_framerate", "led1_mode"] {
        if has(off) {
            set.push(format!("{off}=0"));
        }
    }
    set
}

/// One 32x24 8-bit grayscale frame, after skipping the first frames while the
/// camera settles. Killed if it takes longer than `CAPTURE_TIMEOUT`.
fn capture_gray(device: &str) -> Result<Vec<u8>, String> {
    let filter = format!("select=gte(n\\,5),scale={FRAME_W}:{FRAME_H},format=gray");
    let mut child = Command::new("ffmpeg")
        .args(["-hide_banner", "-loglevel", "error", "-f", "v4l2", "-i", device])
        .args(["-vf", &filter, "-frames:v", "1", "-f", "rawvideo", "-"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("ffmpeg: {e}"))?;

    let started = Instant::now();
    loop {
        match child.try_wait().map_err(|e| e.to_string())? {
            Some(status) => {
                let mut frame = Vec::new();
                if let Some(mut out) = child.stdout.take() {
                    out.read_to_end(&mut frame).map_err(|e| e.to_string())?;
                }
                if !status.success() || frame.len() != FRAME_W * FRAME_H {
                    let mut err = String::new();
                    if let Some(mut e) = child.stderr.take() {
                        let _ = e.read_to_string(&mut err);
                    }
                    return Err(format!("ffmpeg failed ({} bytes): {}", frame.len(), err.trim()));
                }
                return Ok(frame);
            }
            None if started.elapsed() > CAPTURE_TIMEOUT => {
                let _ = child.kill();
                let _ = child.wait();
                return Err("ffmpeg timed out".into());
            }
            None => thread::sleep(Duration::from_millis(100)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NEW_KERNEL: &str = "
User Controls

                     brightness 0x00980900 (int)    : min=0 max=255 step=1 default=128 value=128
                           gain 0x00980913 (int)    : min=0 max=255 step=1 default=0 value=0

Camera Controls

                  auto_exposure 0x009a0901 (menu)   : min=0 max=3 default=3 value=3 (Aperture Priority Mode)
				1: Manual Mode
				3: Aperture Priority Mode
         exposure_time_absolute 0x009a0902 (int)    : min=3 max=2047 step=1 default=250 value=250 flags=inactive
     exposure_dynamic_framerate 0x009a0903 (bool)   : default=0 value=1
";

    #[test]
    fn parses_control_names() {
        let names = control_names(NEW_KERNEL);
        assert_eq!(
            names,
            ["brightness", "gain", "auto_exposure", "exposure_time_absolute", "exposure_dynamic_framerate"]
        );
    }

    #[test]
    fn locks_exposure_with_new_or_old_control_names() {
        let new = exposure_settings(&control_names(NEW_KERNEL), 300);
        assert_eq!(new, ["auto_exposure=1", "exposure_time_absolute=300", "exposure_dynamic_framerate=0"]);

        let old: Vec<String> = ["exposure_auto", "exposure_absolute", "led1_mode"].map(String::from).to_vec();
        assert_eq!(exposure_settings(&old, 300), ["exposure_auto=1", "exposure_absolute=300", "led1_mode=0"]);

        assert!(exposure_settings(&["brightness".to_string()], 300).is_empty());
    }
}
