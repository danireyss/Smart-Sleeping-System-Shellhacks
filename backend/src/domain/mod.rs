pub mod device;
pub mod grounding;
pub mod reading;
pub mod round;
pub mod scoring;
pub mod sleep;
pub mod summary;
pub mod targets;

pub use reading::{Flag, Reading};
pub use scoring::ScoredReading;

use sleep::SleepSession;

/// What GET /api/stream sends: every stored reading, and every sleep-session
/// change (start or end, from the device LCD or the API).
#[derive(Debug, Clone, PartialEq)]
pub enum LiveEvent {
    Reading(ScoredReading),
    /// The session that just started (`ended_at` null) or ended.
    Sleep(SleepSession),
}
