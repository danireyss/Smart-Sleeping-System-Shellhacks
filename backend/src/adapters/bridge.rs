//! Client for the Arduino router (MessagePack-RPC over a Unix socket).
//!
//! Registers `reading` plus the device methods (`sleep_start`, `sleep_end`,
//! `sleep_state`, `score`; see domain/device.rs) and:
//! - forwards every reading the MCU sends to a tokio channel;
//! - answers device calls by asking the device service over another channel
//!   and replying with its result (or an RPC error after 3 s).
//!
//! The socket I/O is blocking, so it runs in `spawn_blocking`. On disconnect or
//! error it waits 2 s, reconnects, and registers again (registrations drop when
//! the client disconnects).
//!
//! `reading` params, in order (see firmware/sensor_bridge.ino):
//!   0. eco2      int    eCO₂ ppm (estimated by the CCS811)
//!   1. tvoc      int    TVOC ppb
//!   2. temp_f    float  °F, NaN if the DHT11 read failed
//!   3. humidity  float  %RH, NaN if the DHT11 read failed
//!   4. uptime_s  int    seconds since the sketch started
//!
//! Numbers are accepted as msgpack ints or floats.

use std::io::{self, BufReader, BufWriter, ErrorKind, Write};
use std::os::unix::net::UnixStream;
use std::sync::mpsc as std_mpsc;
use std::thread;
use std::time::Duration;

use chrono::{SubsecRound, Utc};
use rmpv::Value;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tracing::{debug, error, info, warn};

use crate::domain::device::{DeviceReply, DeviceRequest};
use crate::domain::Reading;

const READING: &str = "reading";
const RECONNECT_DELAY: Duration = Duration::from_secs(2);
/// How long a device call waits for the device service before replying with an error.
const DEVICE_TIMEOUT: Duration = Duration::from_secs(3);

// MessagePack-RPC message types
const REQUEST: u64 = 0;
const RESPONSE: u64 = 1;
const NOTIFICATION: u64 = 2;

/// A device call waiting for an answer from the device service.
pub struct DeviceCall {
    pub request: DeviceRequest,
    pub reply: std_mpsc::Sender<Result<DeviceReply, String>>,
}

/// Where the bridge sends what it receives.
#[derive(Clone)]
pub struct BridgeChannels {
    pub readings: mpsc::Sender<Reading>,
    pub device: mpsc::Sender<DeviceCall>,
}

/// Runs the bridge until the readings receiver is dropped.
pub fn spawn(socket_path: String, channels: BridgeChannels) -> JoinHandle<()> {
    tokio::task::spawn_blocking(move || loop {
        let result = UnixStream::connect(&socket_path)
            .map_err(|e| io::Error::new(e.kind(), format!("connect {socket_path}: {e}")))
            .and_then(|stream| {
                info!("connected to {socket_path}");
                session(stream, &channels)
            });
        match result {
            Ok(SessionEnd::ReceiverClosed) => return,
            Ok(SessionEnd::RouterClosed) => warn!("router closed the connection"),
            Err(e) => error!("bridge: {e}"),
        }
        info!("reconnecting in {}s", RECONNECT_DELAY.as_secs());
        thread::sleep(RECONNECT_DELAY);
    })
}

#[derive(Debug, PartialEq)]
enum SessionEnd {
    RouterClosed,
    ReceiverClosed,
}

/// Methods to register, in order; the register request for method `i` has id `i + 1`.
fn methods() -> Vec<&'static str> {
    std::iter::once(READING).chain(DeviceRequest::ALL.iter().map(|r| r.method())).collect()
}

fn session(stream: UnixStream, channels: &BridgeChannels) -> io::Result<SessionEnd> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut writer = BufWriter::new(stream);

    let methods = methods();
    for (i, method) in methods.iter().enumerate() {
        let register = vec![REQUEST.into(), (i as u64 + 1).into(), "$/register".into(), Value::Array(vec![(*method).into()])];
        send(&mut writer, Value::Array(register))?;
    }

    loop {
        let msg = match rmpv::decode::read_value(&mut reader) {
            Ok(v) => v,
            Err(e) if is_eof(&e) => return Ok(SessionEnd::RouterClosed),
            Err(e) => return Err(io::Error::new(ErrorKind::InvalidData, e.to_string())),
        };
        let Some(parts) = msg.as_array() else {
            warn!("unexpected message: {msg}");
            continue;
        };

        let reading_params = match parts.first().and_then(Value::as_u64) {
            Some(NOTIFICATION) if parts.len() == 3 => {
                (parts[1].as_str() == Some(READING)).then_some(&parts[2])
            }
            Some(REQUEST) if parts.len() == 4 => {
                let id = parts[1].clone();
                match parts[2].as_str() {
                    Some(READING) => {
                        // Acknowledge so a Bridge.call on the MCU doesn't hang.
                        send(&mut writer, response(id, Ok(true.into())))?;
                        Some(&parts[3])
                    }
                    Some(method) => {
                        let result = match DeviceRequest::from_method(method) {
                            Some(request) => ask_device(&channels.device, request),
                            None => Err(format!("unknown method {method}")),
                        };
                        send(&mut writer, response(id, result))?;
                        None
                    }
                    None => None,
                }
            }
            Some(RESPONSE) if parts.len() == 4 => {
                let index = parts[1].as_u64().and_then(|id| id.checked_sub(1));
                if let Some(method) = index.and_then(|i| methods.get(i as usize)) {
                    if parts[2].is_nil() {
                        info!("registered \"{method}\"");
                    } else {
                        return Err(io::Error::other(format!("register {method} failed: {}", parts[2])));
                    }
                }
                None
            }
            _ => None,
        };

        let Some(params) = reading_params else {
            debug!("handled message: {msg}");
            continue;
        };
        match parse_reading(params) {
            Some(reading) => {
                if channels.readings.blocking_send(reading).is_err() {
                    return Ok(SessionEnd::ReceiverClosed);
                }
            }
            None => warn!("could not parse reading params: {params}"),
        }
    }
}

/// Asks the device service and waits (blocking this thread) for the answer.
fn ask_device(device: &mpsc::Sender<DeviceCall>, request: DeviceRequest) -> Result<Value, String> {
    let (reply, answer) = std_mpsc::channel();
    device
        .blocking_send(DeviceCall { request, reply })
        .map_err(|_| "device service is not running".to_string())?;
    match answer.recv_timeout(DEVICE_TIMEOUT) {
        Ok(Ok(DeviceReply::Bool(b))) => Ok(b.into()),
        Ok(Ok(DeviceReply::Float(f))) => Ok(Value::F64(f)),
        Ok(Err(e)) => {
            warn!("device call {} failed: {e}", request.method());
            Err(e)
        }
        Err(_) => Err(format!("{} timed out", request.method())),
    }
}

/// `[1, id, error, result]`
fn response(id: Value, result: Result<Value, String>) -> Value {
    let (error, value) = match result {
        Ok(v) => (Value::Nil, v),
        Err(e) => (e.into(), Value::Nil),
    };
    Value::Array(vec![RESPONSE.into(), id, error, value])
}

/// Parses `[eco2, tvoc, temp_f, humidity, uptime_s]`, timestamped now (UTC, to the
/// millisecond, matching storage precision).
fn parse_reading(params: &Value) -> Option<Reading> {
    let p = params.as_array()?;
    if p.len() != 5 {
        return None;
    }
    let uptime = number(&p[4])?;
    if !uptime.is_finite() || uptime < 0.0 {
        return None;
    }
    Some(Reading {
        received_at: Utc::now().trunc_subsecs(3),
        eco2_ppm: number(&p[0])?,
        tvoc_ppb: number(&p[1])?,
        temp_f: number(&p[2]).filter(|v| !v.is_nan()),
        humidity_pct: number(&p[3]).filter(|v| !v.is_nan()),
        uptime_s: uptime as u64,
    })
}

/// A msgpack int or float as f64.
fn number(v: &Value) -> Option<f64> {
    match v {
        Value::Integer(i) => i.as_f64(),
        // Go through the shortest decimal form so 49.8f32 becomes 49.8, not 49.7999992...
        Value::F32(f) => f.to_string().parse().ok(),
        Value::F64(f) => Some(*f),
        _ => None,
    }
}

fn send(writer: &mut impl Write, msg: Value) -> io::Result<()> {
    rmpv::encode::write_value(writer, &msg)?;
    writer.flush()
}

fn is_eof(e: &rmpv::decode::Error) -> bool {
    use rmpv::decode::Error::*;
    match e {
        InvalidMarkerRead(io) | InvalidDataRead(io) => io.kind() == ErrorKind::UnexpectedEof,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params(values: Vec<Value>) -> Value {
        Value::Array(values)
    }

    #[test]
    fn parses_ints_and_floats() {
        let r = parse_reading(&params(vec![
            612.into(),
            45.into(),
            Value::F32(68.5),
            Value::F64(47.0),
            1300.into(),
        ]))
        .unwrap();
        assert_eq!(r.eco2_ppm, 612.0);
        assert_eq!(r.tvoc_ppb, 45.0);
        assert_eq!(r.temp_f, Some(68.5));
        assert_eq!(r.humidity_pct, Some(47.0));
        assert_eq!(r.uptime_s, 1300);

        // Numbers may arrive as the other kind too.
        let r = parse_reading(&params(vec![
            Value::F32(612.0),
            45.into(),
            68.into(),
            47.into(),
            Value::F64(1300.0),
        ]))
        .unwrap();
        assert_eq!(r.eco2_ppm, 612.0);
        assert_eq!(r.temp_f, Some(68.0));
        assert_eq!(r.uptime_s, 1300);
    }

    #[test]
    fn f32_values_keep_their_decimal_form() {
        assert_eq!(number(&Value::F32(49.8)), Some(49.8));
        assert_eq!(number(&Value::F32(77.72)), Some(77.72));
        assert!(number(&Value::F32(f32::NAN)).unwrap().is_nan());
    }

    #[test]
    fn nan_dht_values_become_none() {
        let r = parse_reading(&params(vec![
            400.into(),
            0.into(),
            Value::F32(f32::NAN),
            Value::F32(f32::NAN),
            5.into(),
        ]))
        .unwrap();
        assert_eq!(r.temp_f, None);
        assert_eq!(r.humidity_pct, None);
    }

    #[test]
    fn rejects_wrong_shape() {
        let four = params(vec![400.into(), 0.into(), 68.into(), 47.into()]);
        assert!(parse_reading(&four).is_none());
        let text = params(vec!["400".into(), 0.into(), 68.into(), 47.into(), 5.into()]);
        assert!(parse_reading(&text).is_none());
        let negative_uptime = params(vec![400.into(), 0.into(), 68.into(), 47.into(), (-1).into()]);
        assert!(parse_reading(&negative_uptime).is_none());
    }

    /// Plays the router on one end of a socket pair.
    struct FakeRouter {
        stream: UnixStream,
    }

    impl FakeRouter {
        fn read(&mut self) -> Value {
            rmpv::decode::read_value(&mut self.stream).unwrap()
        }
        fn write(&mut self, msg: Value) {
            rmpv::encode::write_value(&mut self.stream, &msg).unwrap();
        }
    }

    fn arr(items: Vec<Value>) -> Value {
        Value::Array(items)
    }

    #[test]
    fn registers_forwards_readings_and_answers_device_calls() {
        let (ours, theirs) = UnixStream::pair().unwrap();
        let (readings_tx, mut readings_rx) = mpsc::channel(8);
        let (device_tx, mut device_rx) = mpsc::channel(8);
        let channels = BridgeChannels { readings: readings_tx, device: device_tx };
        let bridge = thread::spawn(move || session(ours, &channels));
        let mut router = FakeRouter { stream: theirs };

        // Registers every method, then accepts the router's replies.
        let names = ["reading", "sleep_start", "sleep_end", "sleep_state", "score"];
        for (i, name) in names.iter().enumerate() {
            let msg = router.read();
            assert_eq!(msg, arr(vec![0.into(), (i as u64 + 1).into(), "$/register".into(), arr(vec![(*name).into()])]));
            router.write(arr(vec![1.into(), (i as u64 + 1).into(), Value::Nil, true.into()]));
        }

        // A reading notification is forwarded.
        let params = arr(vec![477.into(), 11.into(), Value::F32(76.64), Value::F32(49.8), 2000.into()]);
        router.write(arr(vec![2.into(), "reading".into(), params]));
        let reading = readings_rx.blocking_recv().unwrap();
        assert_eq!((reading.eco2_ppm, reading.temp_f, reading.uptime_s), (477.0, Some(76.64), 2000));

        // Device calls are answered with the device service's reply.
        let mut call = |id: u64, method: &str, answer: Result<DeviceReply, String>| {
            router.write(arr(vec![0.into(), id.into(), method.into(), arr(vec![])]));
            let asked = device_rx.blocking_recv().unwrap();
            asked.reply.send(answer).unwrap();
            (asked.request, router.read())
        };
        let (req, resp) = call(42, "sleep_start", Ok(DeviceReply::Bool(true)));
        assert_eq!(req, DeviceRequest::SleepStart);
        assert_eq!(resp, arr(vec![1.into(), 42.into(), Value::Nil, true.into()]));

        let (req, resp) = call(43, "score", Ok(DeviceReply::Float(78.0)));
        assert_eq!(req, DeviceRequest::Score);
        assert_eq!(resp, arr(vec![1.into(), 43.into(), Value::Nil, Value::F64(78.0)]));

        let (_, resp) = call(44, "sleep_state", Err("database is locked".into()));
        assert_eq!(resp, arr(vec![1.into(), 44.into(), "database is locked".into(), Value::Nil]));

        // Unknown methods get an error without bothering the device service.
        router.write(arr(vec![0.into(), 45.into(), "reboot".into(), arr(vec![])]));
        assert_eq!(router.read(), arr(vec![1.into(), 45.into(), "unknown method reboot".into(), Value::Nil]));

        drop(router);
        assert_eq!(bridge.join().unwrap().unwrap(), SessionEnd::RouterClosed);
    }

    #[test]
    fn failed_registration_ends_the_session() {
        let (ours, theirs) = UnixStream::pair().unwrap();
        let (readings_tx, _readings_rx) = mpsc::channel(8);
        let (device_tx, _device_rx) = mpsc::channel(8);
        let channels = BridgeChannels { readings: readings_tx, device: device_tx };
        let bridge = thread::spawn(move || session(ours, &channels));
        let mut router = FakeRouter { stream: theirs };
        for _ in 0..5 {
            router.read();
        }
        router.write(arr(vec![1.into(), 2.into(), "already registered".into(), Value::Nil]));
        let err = bridge.join().unwrap().unwrap_err();
        assert!(err.to_string().contains("register sleep_start failed"), "{err}");
    }
}
