//! Calls the device (the MCU sketch driving the touch LCD) makes to the backend
//! with `Bridge.call(method).result(value)`. Pure types, no I/O.
//!
//! | method        | when                      | effect                            | returns |
//! | ------------- | ------------------------- | --------------------------------- | ------- |
//! | `sleep_start` | tap on the LCD button     | start a session (or keep the open one) | `true`  |
//! | `sleep_end`   | long-press on the LCD     | end the open session, if any      | `false` |
//! | `sleep_state` | periodically              | none                              | `true` while a session is open |
//! | `score`       | periodically              | none                              | latest total score, or `-1` when flagged/missing |
//!
//! Start and end are separate (not a toggle) so a lost reply can never flip the
//! state the wrong way, and `sleep_state` keeps the LCD in sync when sessions
//! are started or ended through the API.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceRequest {
    SleepStart,
    SleepEnd,
    SleepState,
    Score,
}

impl DeviceRequest {
    pub const ALL: [DeviceRequest; 4] = [
        DeviceRequest::SleepStart,
        DeviceRequest::SleepEnd,
        DeviceRequest::SleepState,
        DeviceRequest::Score,
    ];

    /// The RPC method name the sketch calls.
    pub fn method(self) -> &'static str {
        match self {
            DeviceRequest::SleepStart => "sleep_start",
            DeviceRequest::SleepEnd => "sleep_end",
            DeviceRequest::SleepState => "sleep_state",
            DeviceRequest::Score => "score",
        }
    }

    pub fn from_method(method: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|r| r.method() == method)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DeviceReply {
    Bool(bool),
    Float(f64),
}

/// `score` reply when there is no current score (no reading, or it is flagged).
pub const NO_SCORE: f64 = -1.0;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn methods_round_trip() {
        for r in DeviceRequest::ALL {
            assert_eq!(DeviceRequest::from_method(r.method()), Some(r));
        }
        assert_eq!(DeviceRequest::from_method("reading"), None);
    }
}
