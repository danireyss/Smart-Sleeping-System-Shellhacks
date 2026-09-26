//! Receives readings from the bridge: validates, stores, scores, and publishes
//! them to live subscribers (the SSE stream).

use std::sync::Arc;

use tokio::sync::broadcast;
use tracing::{error, info, warn};

use crate::domain::{Reading, ScoredReading};
use crate::repositories::ReadingRepository;

pub struct IngestService {
    repo: Arc<dyn ReadingRepository>,
    events: broadcast::Sender<ScoredReading>,
}

impl IngestService {
    pub fn new(repo: Arc<dyn ReadingRepository>, events: broadcast::Sender<ScoredReading>) -> Self {
        Self { repo, events }
    }

    pub fn handle(&self, reading: Reading) {
        let scored = ScoredReading::from(reading);
        log(&scored);
        if let Err(e) = self.repo.save(&scored.reading, &scored.flags) {
            error!("failed to store reading: {e}");
            return;
        }
        // An error only means nobody is subscribed right now.
        let _ = self.events.send(scored);
    }
}

fn log(scored: &ScoredReading) {
    let r = &scored.reading;
    let summary = format!(
        "{} eCO2 (estimated) {} ppm, TVOC {} ppb, temp {}, RH {}, uptime {}s",
        r.received_at.format("%Y-%m-%dT%H:%M:%SZ"),
        r.eco2_ppm,
        r.tvoc_ppb,
        fmt_opt(r.temp_f, "F"),
        fmt_opt(r.humidity_pct, "%"),
        r.uptime_s,
    );
    match &scored.score {
        Some(s) => info!(
            "reading {summary} score {:.1} {:?} (eCO2 {:.0}, temp {:.0}, RH {:.0})",
            s.total, s.band, s.eco2, s.temp, s.humidity
        ),
        None => {
            let names: Vec<_> = scored.flags.iter().map(|f| f.as_str()).collect();
            warn!("reading {summary} not scored, flagged [{}]", names.join(", "));
        }
    }
}

fn fmt_opt(v: Option<f64>, unit: &str) -> String {
    v.map_or_else(|| "missing".to_string(), |v| format!("{v:.1} {unit}"))
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use chrono::{DateTime, Utc};

    use super::*;
    use crate::domain::reading::WARM_UP_SECS;
    use crate::domain::Flag;
    use crate::repositories::RepoError;

    #[derive(Default)]
    struct FakeRepo {
        saved: Mutex<Vec<(Reading, Vec<Flag>)>>,
        fail: bool,
    }

    impl ReadingRepository for FakeRepo {
        fn save(&self, reading: &Reading, flags: &[Flag]) -> Result<i64, RepoError> {
            if self.fail {
                return Err("disk full".into());
            }
            let mut saved = self.saved.lock().unwrap();
            saved.push((reading.clone(), flags.to_vec()));
            Ok(saved.len() as i64)
        }

        fn latest(&self) -> Result<Option<Reading>, RepoError> {
            unimplemented!()
        }

        fn range(&self, _: DateTime<Utc>, _: DateTime<Utc>) -> Result<Vec<Reading>, RepoError> {
            unimplemented!()
        }
    }

    fn valid() -> Reading {
        Reading {
            received_at: Utc::now(),
            eco2_ppm: 450.0,
            tvoc_ppb: 5.0,
            temp_f: Some(68.0),
            humidity_pct: Some(45.0),
            uptime_s: WARM_UP_SECS,
        }
    }

    #[test]
    fn saves_and_publishes_every_reading_with_its_flags() {
        let repo = Arc::new(FakeRepo::default());
        let (tx, mut rx) = broadcast::channel(8);
        let ingest = IngestService::new(repo.clone(), tx);
        let ok = valid();
        let warming = Reading { uptime_s: 10, ..ok.clone() };

        ingest.handle(ok.clone());
        ingest.handle(warming.clone());

        let saved = repo.saved.lock().unwrap();
        assert_eq!(*saved, vec![(ok, vec![]), (warming.clone(), vec![Flag::WarmUp])]);

        let first = rx.try_recv().unwrap();
        assert!(first.flags.is_empty());
        assert_eq!(first.score.unwrap().total, 100.0);
        let second = rx.try_recv().unwrap();
        assert_eq!(second, ScoredReading { reading: warming, flags: vec![Flag::WarmUp], score: None });
    }

    #[test]
    fn does_not_publish_when_save_fails() {
        let repo = Arc::new(FakeRepo { fail: true, ..Default::default() });
        let (tx, mut rx) = broadcast::channel(8);
        IngestService::new(repo, tx).handle(valid());
        assert!(rx.try_recv().is_err());
    }
}
