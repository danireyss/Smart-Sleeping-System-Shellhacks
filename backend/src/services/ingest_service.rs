//! Receives readings from the bridge: validates, stores, and scores them.
//! The SSE broadcast gets added here next.

use std::sync::Arc;

use tracing::{error, info, warn};

use crate::domain::scoring;
use crate::domain::Reading;
use crate::repositories::ReadingRepository;

pub struct IngestService {
    repo: Arc<dyn ReadingRepository>,
}

impl IngestService {
    pub fn new(repo: Arc<dyn ReadingRepository>) -> Self {
        Self { repo }
    }

    pub fn handle(&self, reading: Reading) {
        let flags = reading.flags();
        if let Err(e) = self.repo.save(&reading, &flags) {
            error!("failed to store reading: {e}");
        }

        let summary = format!(
            "{} eCO2 (estimated) {} ppm, TVOC {} ppb, temp {}, RH {}, uptime {}s",
            reading.received_at.format("%Y-%m-%dT%H:%M:%SZ"),
            reading.eco2_ppm,
            reading.tvoc_ppb,
            fmt_opt(reading.temp_f, "F"),
            fmt_opt(reading.humidity_pct, "%"),
            reading.uptime_s,
        );
        match scoring::score(&reading) {
            Some(s) => info!(
                "reading {summary} score {:.1} {:?} (eCO2 {:.0}, temp {:.0}, RH {:.0})",
                s.total, s.band, s.eco2, s.temp, s.humidity
            ),
            None => {
                let names: Vec<_> = flags.iter().map(|f| f.as_str()).collect();
                warn!("reading {summary} not scored, flagged [{}]", names.join(", "));
            }
        }
    }
}

fn fmt_opt(v: Option<f64>, unit: &str) -> String {
    v.map_or_else(|| "missing".to_string(), |v| format!("{v:.1} {unit}"))
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use chrono::Utc;

    use super::*;
    use crate::domain::reading::WARM_UP_SECS;
    use crate::domain::Flag;
    use crate::repositories::RepoError;

    #[derive(Default)]
    struct FakeRepo {
        saved: Mutex<Vec<(Reading, Vec<Flag>)>>,
    }

    impl ReadingRepository for FakeRepo {
        fn save(&self, reading: &Reading, flags: &[Flag]) -> Result<i64, RepoError> {
            let mut saved = self.saved.lock().unwrap();
            saved.push((reading.clone(), flags.to_vec()));
            Ok(saved.len() as i64)
        }
    }

    #[test]
    fn saves_every_reading_with_its_flags() {
        let repo = Arc::new(FakeRepo::default());
        let ingest = IngestService::new(repo.clone());
        let valid = Reading {
            received_at: Utc::now(),
            eco2_ppm: 450.0,
            tvoc_ppb: 5.0,
            temp_f: Some(68.0),
            humidity_pct: Some(45.0),
            uptime_s: WARM_UP_SECS,
        };
        let warming = Reading { uptime_s: 10, ..valid.clone() };

        ingest.handle(valid.clone());
        ingest.handle(warming.clone());

        let saved = repo.saved.lock().unwrap();
        assert_eq!(*saved, vec![(valid, vec![]), (warming, vec![Flag::WarmUp])]);
    }
}
