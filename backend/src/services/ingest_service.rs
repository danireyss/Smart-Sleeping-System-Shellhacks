//! Receives readings from the bridge: attaches the latest webcam light/sound
//! values, validates, stores, scores, and publishes them to live subscribers
//! (the SSE stream).

use std::sync::Arc;

use tokio::sync::broadcast;
use tracing::{error, info, warn};

use super::AmbientService;
use crate::domain::{LiveEvent, Reading, ScoredReading};
use crate::repositories::ReadingRepository;

pub struct IngestService {
    repo: Arc<dyn ReadingRepository>,
    events: broadcast::Sender<LiveEvent>,
    ambient: Arc<AmbientService>,
}

impl IngestService {
    pub fn new(
        repo: Arc<dyn ReadingRepository>,
        events: broadcast::Sender<LiveEvent>,
        ambient: Arc<AmbientService>,
    ) -> Self {
        Self { repo, events, ambient }
    }

    pub fn handle(&self, mut reading: Reading) {
        reading.ambient = self.ambient.current(reading.received_at);
        let scored = ScoredReading::from(reading);
        log(&scored);
        if let Err(e) = self.repo.save(&scored.reading, &scored.flags) {
            error!("failed to store reading: {e}");
            return;
        }
        // An error only means nobody is subscribed right now.
        let _ = self.events.send(LiveEvent::Reading(scored));
    }
}

fn log(scored: &ScoredReading) {
    let r = &scored.reading;
    let summary = format!(
        "{} eCO2 (estimated) {} ppm, TVOC {} ppb, temp {}, RH {}, light {}, sound {}, uptime {}s",
        r.received_at.format("%Y-%m-%dT%H:%M:%SZ"),
        r.eco2_ppm,
        r.tvoc_ppb,
        fmt_opt(r.temp_f, "F"),
        fmt_opt(r.humidity_pct, "%"),
        fmt_opt(r.ambient.light_level, "/100"),
        fmt_opt(r.ambient.sound_db, "dB"),
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
    use crate::domain::ambient::Ambient;
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
            ambient: Ambient::default(),
        }
    }

    #[test]
    fn saves_and_publishes_every_reading_with_its_flags() {
        let repo = Arc::new(FakeRepo::default());
        let (tx, mut rx) = broadcast::channel(8);
        let ingest = IngestService::new(repo.clone(), tx, Arc::default());
        let ok = valid();
        let warming = Reading { uptime_s: 10, ..ok.clone() };

        ingest.handle(ok.clone());
        ingest.handle(warming.clone());

        let saved = repo.saved.lock().unwrap();
        assert_eq!(*saved, vec![(ok, vec![]), (warming.clone(), vec![Flag::WarmUp])]);

        let LiveEvent::Reading(first) = rx.try_recv().unwrap() else { panic!("expected a reading") };
        assert!(first.flags.is_empty());
        assert_eq!(first.score.unwrap().total, 100.0);
        let second = rx.try_recv().unwrap();
        let expected = ScoredReading { reading: warming, flags: vec![Flag::WarmUp], score: None };
        assert_eq!(second, LiveEvent::Reading(expected));
    }

    #[test]
    fn attaches_fresh_light_and_sound() {
        let repo = Arc::new(FakeRepo::default());
        let (tx, _rx) = broadcast::channel(8);
        let ambient = Arc::new(AmbientService::default());
        let r = valid();
        ambient.record_light(3.0, r.received_at - chrono::Duration::seconds(30));
        ambient.record_sound(33.0, 41.0, r.received_at - chrono::Duration::seconds(300)); // stale
        IngestService::new(repo.clone(), tx, ambient).handle(r);
        let saved = &repo.saved.lock().unwrap()[0].0;
        assert_eq!(saved.ambient.light_level, Some(3.0));
        assert_eq!(saved.ambient.sound_db, None);
    }

    #[test]
    fn does_not_publish_when_save_fails() {
        let repo = Arc::new(FakeRepo { fail: true, ..Default::default() });
        let (tx, mut rx) = broadcast::channel(8);
        IngestService::new(repo, tx, Arc::default()).handle(valid());
        assert!(rx.try_recv().is_err());
    }
}
