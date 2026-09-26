//! Connects to the Arduino router, registers the "reading" method, and prints
//! every reading the MCU sends via `Bridge.notify` / `Bridge.call`.
//!
//! Usage: router-test [SOCKET_PATH]   (default: /var/run/arduino-router.sock,
//! or the ROUTER_SOCKET env var)

use std::io::{self, BufReader, BufWriter, ErrorKind, Write};
use std::os::unix::net::UnixStream;

use rmpv::Value;

const DEFAULT_SOCKET: &str = "/var/run/arduino-router.sock";
const METHOD: &str = "reading";

// MessagePack-RPC message types
const REQUEST: u64 = 0;
const RESPONSE: u64 = 1;
const NOTIFICATION: u64 = 2;

fn main() {
    let path = std::env::args()
        .nth(1)
        .or_else(|| std::env::var("ROUTER_SOCKET").ok())
        .unwrap_or_else(|| DEFAULT_SOCKET.to_string());

    if let Err(e) = run(&path) {
        eprintln!("error: {e}");
        if e.kind() == ErrorKind::PermissionDenied {
            eprintln!("hint: run with sudo or add your user to the socket's group");
        }
        std::process::exit(1);
    }
}

fn run(path: &str) -> io::Result<()> {
    let stream = UnixStream::connect(path)
        .map_err(|e| io::Error::new(e.kind(), format!("connect {path}: {e}")))?;
    println!("connected to {path}");

    let mut reader = BufReader::new(stream.try_clone()?);
    let mut writer = BufWriter::new(stream);

    let register_id = 1u32;
    send(
        &mut writer,
        &Value::Array(vec![
            REQUEST.into(),
            register_id.into(),
            "$/register".into(),
            Value::Array(vec![METHOD.into()]),
        ]),
    )?;
    println!("sent $/register [\"{METHOD}\"], waiting for messages...");

    loop {
        let msg = match rmpv::decode::read_value(&mut reader) {
            Ok(v) => v,
            Err(e) if is_eof(&e) => {
                println!("router closed the connection");
                return Ok(());
            }
            Err(e) => return Err(io::Error::new(ErrorKind::InvalidData, e.to_string())),
        };
        handle(&mut writer, msg)?;
    }
}

fn handle(writer: &mut impl Write, msg: Value) -> io::Result<()> {
    let Some(parts) = msg.as_array() else {
        println!("unexpected message: {msg}");
        return Ok(());
    };

    match parts.first().and_then(Value::as_u64) {
        Some(REQUEST) if parts.len() == 4 => {
            let id = parts[1].clone();
            let method = parts[2].as_str().unwrap_or("?");
            print_call("request", method, &parts[3]);
            // Always acknowledge so the MCU's Bridge.call doesn't hang.
            send(
                writer,
                &Value::Array(vec![RESPONSE.into(), id, Value::Nil, Value::Boolean(true)]),
            )?;
        }
        Some(RESPONSE) if parts.len() == 4 => {
            let (id, err, result) = (&parts[1], &parts[2], &parts[3]);
            if err.is_nil() {
                println!("response id={id}: ok, result={result}");
            } else {
                println!("response id={id}: ERROR {err}");
            }
        }
        Some(NOTIFICATION) if parts.len() == 3 => {
            let method = parts[1].as_str().unwrap_or("?");
            print_call("notification", method, &parts[2]);
        }
        _ => println!("unrecognized message: {msg}"),
    }
    Ok(())
}

fn print_call(kind: &str, method: &str, params: &Value) {
    if method != METHOD {
        println!("{kind} {method}: {params}");
        return;
    }
    match parse_reading(params) {
        Some((eco2, tvoc, temp_f, humidity, uptime_s)) => println!(
            "reading: eCO2 (estimated) {eco2} ppm, TVOC {tvoc} ppb, {temp_f:.1} F, {humidity:.1} %RH, uptime {uptime_s} s"
        ),
        None => println!("{kind} {method} (unparsed): {params}"),
    }
}

/// Params are `[eco2, tvoc, temp_f, humidity, uptime_s]` as sent by firmware/sensor_bridge.ino.
fn parse_reading(params: &Value) -> Option<(i64, i64, f64, f64, i64)> {
    let p = params.as_array()?;
    if p.len() != 5 {
        return None;
    }
    Some((p[0].as_i64()?, p[1].as_i64()?, as_f64(&p[2])?, as_f64(&p[3])?, p[4].as_i64()?))
}

// rmpv::Value::as_f64 handles F32/F64; also accept integers in case the MCU sends whole numbers.
fn as_f64(v: &Value) -> Option<f64> {
    v.as_f64().or_else(|| v.as_i64().map(|i| i as f64))
}

fn send(writer: &mut impl Write, msg: &Value) -> io::Result<()> {
    rmpv::encode::write_value(writer, msg)?;
    writer.flush()
}

fn is_eof(e: &rmpv::decode::Error) -> bool {
    use rmpv::decode::Error::*;
    match e {
        InvalidMarkerRead(io) | InvalidDataRead(io) => io.kind() == ErrorKind::UnexpectedEof,
        _ => false,
    }
}
