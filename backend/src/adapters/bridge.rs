//! Client for the Arduino router (MessagePack-RPC over a Unix socket).
//!
//! Registers the `reading` method and forwards every reading the MCU sends to a
//! tokio channel. The socket I/O is blocking, so it runs in `spawn_blocking`.
//! On disconnect or error it waits 2 s, reconnects, and registers again
//! (registrations drop when the client disconnects).
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
use std::thread;
use std::time::Duration;

use chrono::Utc;
use rmpv::Value;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tracing::{debug, error, info, warn};

use crate::domain::Reading;

const METHOD: &str = "reading";
const RECONNECT_DELAY: Duration = Duration::from_secs(2);
const REGISTER_ID: u32 = 1;

// MessagePack-RPC message types
const REQUEST: u64 = 0;
const RESPONSE: u64 = 1;
const NOTIFICATION: u64 = 2;

/// Runs the bridge until the receiving side of `tx` is dropped.
pub fn spawn(socket_path: String, tx: mpsc::Sender<Reading>) -> JoinHandle<()> {
    tokio::task::spawn_blocking(move || loop {
        match session(&socket_path, &tx) {
            Ok(SessionEnd::ReceiverClosed) => return,
            Ok(SessionEnd::RouterClosed) => warn!("router closed the connection"),
            Err(e) => error!("bridge: {e}"),
        }
        info!("reconnecting in {}s", RECONNECT_DELAY.as_secs());
        thread::sleep(RECONNECT_DELAY);
    })
}

enum SessionEnd {
    RouterClosed,
    ReceiverClosed,
}

fn session(path: &str, tx: &mpsc::Sender<Reading>) -> io::Result<SessionEnd> {
    let stream = UnixStream::connect(path)
        .map_err(|e| io::Error::new(e.kind(), format!("connect {path}: {e}")))?;
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut writer = BufWriter::new(stream);
    info!("connected to {path}");

    send(
        &mut writer,
        Value::Array(vec![
            REQUEST.into(),
            REGISTER_ID.into(),
            "$/register".into(),
            Value::Array(vec![METHOD.into()]),
        ]),
    )?;

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

        let params = match parts.first().and_then(Value::as_u64) {
            Some(NOTIFICATION) if parts.len() == 3 => {
                (parts[1].as_str() == Some(METHOD)).then_some(&parts[2])
            }
            Some(REQUEST) if parts.len() == 4 => {
                // Acknowledge so a Bridge.call on the MCU doesn't hang.
                let reply = vec![RESPONSE.into(), parts[1].clone(), Value::Nil, true.into()];
                send(&mut writer, Value::Array(reply))?;
                (parts[2].as_str() == Some(METHOD)).then_some(&parts[3])
            }
            Some(RESPONSE) if parts.len() == 4 => {
                if parts[1].as_u64() == Some(REGISTER_ID.into()) {
                    if parts[2].is_nil() {
                        info!("registered \"{METHOD}\"");
                    } else {
                        return Err(io::Error::other(format!("register failed: {}", parts[2])));
                    }
                }
                None
            }
            _ => None,
        };

        let Some(params) = params else {
            debug!("ignored message: {msg}");
            continue;
        };
        match parse_reading(params) {
            Some(reading) => {
                if tx.blocking_send(reading).is_err() {
                    return Ok(SessionEnd::ReceiverClosed);
                }
            }
            None => warn!("could not parse reading params: {params}"),
        }
    }
}

/// Parses `[eco2, tvoc, temp_f, humidity, uptime_s]`, timestamped now (UTC).
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
        received_at: Utc::now(),
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
}
