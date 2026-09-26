//! Receives readings from the bridge and validates them.
//! Storage, scoring, and the SSE broadcast get added here next.

use tracing::{info, warn};

use crate::domain::Reading;

#[derive(Default)]
pub struct IngestService;

impl IngestService {
    pub fn handle(&self, reading: Reading) {
        let flags = reading.flags();
        let summary = format!(
            "{} eCO2 (estimated) {} ppm, TVOC {} ppb, temp {}, RH {}, uptime {}s",
            reading.received_at.format("%Y-%m-%dT%H:%M:%SZ"),
            reading.eco2_ppm,
            reading.tvoc_ppb,
            fmt_opt(reading.temp_f, "F"),
            fmt_opt(reading.humidity_pct, "%"),
            reading.uptime_s,
        );
        if flags.is_empty() {
            info!("reading {summary}");
        } else {
            warn!("reading {summary} flagged {flags:?}");
        }
    }
}

fn fmt_opt(v: Option<f64>, unit: &str) -> String {
    v.map_or_else(|| "missing".to_string(), |v| format!("{v:.1} {unit}"))
}
