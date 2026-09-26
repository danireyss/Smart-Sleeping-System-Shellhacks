# Sleep Environment Intelligence System — Project Context

Hackathon project (in-person MLH event, started Fri Sep 25 2026). Measures bedroom
eCO₂, temperature, and humidity every minute on an Arduino UNO Q, turns them into a
sleep-environment score, and lets the user chat with an AI agent that explains the
results. Scores the **room, not the person** — no health data.

## Current status (Sat Sep 26, ~1:00 AM)

- UNO Q set up (App Lab 0.10.0, board name `danmig`, SSH works: `ssh arduino@danmig.local`).
- Rust installed on the board; `build-essential pkg-config libssl-dev` installed.
- Both sensors wired and **verified** with a C++ test sketch in App Lab (app `sensor-test`):
  CCS811 found at I²C **0x5A**; DHT11 reads temp/humidity.
- **Next step (untested):** sketch sends readings to Linux via `Bridge.notify("reading", ...)`,
  and a Rust program on Linux receives them through the Arduino router socket.
  See "Next step" below.

## Key decisions (these override anything older in the SRS)

- **Hybrid architecture.** MCU firmware stays C++ (App Lab sketch + official
  `Arduino_RouterBridge`). Everything on Linux is Rust. Embedded Rust on the MCU was
  dropped: the only community framework (FeurJak/DragonWing-rs) has no sensor/I²C
  support, needs Docker + Zephyr SDK, and replaces the official router. Embedded Rust
  is a post-hackathon upgrade.
- **Backend:** Rust, Tokio, **Axum**, layered architecture (controllers → services →
  repositories), Cargo workspace.
- **Storage:** SQLite (not a time-series DB — ~1,440 rows/night).
- **Live updates:** Server-Sent Events (SSE), not WebSocket. Chat messages are HTTP POST;
  agent replies stream over SSE.
- **AI:** Groq free tier via `async-openai` (OpenAI-compatible). Base URL, key, and model
  in config/env so switching to Gemini is config-only. No LLM gateway. Tool calling:
  the backend calls the model; the model requests tools; the backend runs them in-process
  via services and sends results back. The model never touches the DB or hardware.
- **Deployment:** everything runs locally on the UNO Q; only the AI call goes to the cloud.
  Demo over phone hotspot / travel router. Optional Cloudflare Tunnel for judges.
- **No policy engine / actuator** unless the core pipeline is done (stretch goal).

## Hardware

Arduino UNO Q (STM32U585 MCU on Zephyr + Qualcomm QRB2210 Linux, Debian).
Powered over USB-C only (no barrel jack).

| Sensor pin | UNO Q pin | Note |
| --- | --- | --- |
| CCS811 VCC | 3.3V | power header |
| CCS811 GND | GND | power header |
| CCS811 WAKE | GND | **required**, second GND on power header |
| CCS811 SDA / SCL | SDA / SCL | top-left header by AREF |
| CCS811 INT, RST | not connected | |
| DHT11 V | digital pin 7 | set OUTPUT HIGH in code (only one 3.3V pin) |
| DHT11 S | digital pin 2 | data |
| DHT11 G | GND | next to pin 13 |

Sensor caveats:
- CCS811 **estimates** CO₂ from VOCs (eCO₂). Floor is 400 ppm; ~20 min warm-up; breath
  barely registers. For demos, hand sanitizer near it spikes eCO₂/TVOC. Always label as
  "eCO₂ (estimated)" in UI and agent replies. Pass DHT11 temp/humidity to
  `ccs.setEnvironmentalData(h, c)` for compensation.
- DHT11 is coarse (±2 °C, ±5% RH). Keep it a few inches from the board (board heat).

## App Lab / MCU gotchas (learned the hard way)

- Use `Monitor.begin()` / `Monitor.println()` (from `Arduino_RouterBridge.h`), not
  `Serial` — Serial printed nothing on this board.
- Libraries must be added **per app** under "Sketch Libraries" in App Lab or the sketch
  silently fails to compile/upload. Used: "DHT sensor library" (Adafruit),
  "Adafruit Unified Sensor", "Adafruit CCS811 Library".
- Examples are read-only; use "Copy and edit app".
- `LED_BUILTIN` is active-low.
- Paste in the App Lab terminal with right-click, not Ctrl+V.

## Arduino router protocol (Linux side)

The router (`arduino-router`, Go) is a MessagePack-RPC hub. Clients connect over a Unix
socket (path TBD — find with `sudo ss -xlp | grep -i router`; likely
`/var/run/arduino-router.sock`).
- Register a method: `[0, msgid, "$/register", ["reading"]]` → `[1, msgid, null, true]`
- Requests: `[0, msgid, method, params]`, responses: `[1, msgid, error, result]`,
  notifications: `[2, method, params]`.
- Registrations drop when the client disconnects. `$/unregister`, `$/reset` also exist.

## Next step (untested)

1. MCU sketch (App Lab app `sensor-test`, see `firmware/sensor_bridge.ino`) keeps the
   latest CCS811 values and every 10 s (60 s in production) reads the DHT11, sets
   compensation, and calls
   `Bridge.notify("reading", (int)eco2, (int)tvoc, temp_f, humidity)`.
2. Rust test program (`router-test/`, crate `rmpv`): connect to the router socket, send
   `$/register` for `reading`, loop reading msgpack values and print notifications;
   reply to any requests with `[1, id, nil, true]`.
3. If registration works but no readings arrive, switch the sketch to `Bridge.call`.
4. If "Permission denied" on the socket, run with sudo or fix the socket group.

## Backend layout (target)

```
sleep-env/
├── protocol/            # shared Reading type
└── backend/src/
    ├── main.rs          # wiring: repos → services → router, spawn tasks
    ├── config.rs        # targets, intervals, AI base URL/key/model from env
    ├── domain/          # pure types + scoring math (unit-tested, no I/O)
    ├── controllers/     # readings.rs, stream.rs (SSE), chat.rs
    ├── services/        # ingest_service, reading_service, agent_service
    ├── repositories/    # ReadingRepository trait + sqlite_repo.rs
    └── adapters/        # bridge.rs (router client), llm_client.rs
```

Flow: bridge.rs → ingest_service (validate, store, score) → Tokio broadcast channel →
SSE clients. Agent tools call reading_service (same path as the dashboard).

API: `GET /api/current`, `GET /api/summary?start=&end=`, `GET /api/night/:date`,
`GET /api/stream` (SSE), `POST /api/chat`, `GET /` (frontend).

Crates: tokio, axum, sqlx or rusqlite, serde, rmpv/rmp-serde, async-openai, chrono, tracing.

Build order: bridge + ingest printing readings → storage → scoring with tests →
API + SSE → frontend → agent.

Builds happen on the board (aarch64 Debian, slow first build). Use debug builds while
developing, `--release` for the overnight run and demo.

## Scoring

Per-minute score = average of three 0–100 sub-scores; nightly score = average of minute
scores in the sleep window (user-set bedtime/wake time). Exclude flagged/missing minutes;
a night with <60% valid minutes is "incomplete".

| Metric | 100 points | Outside | 0 points at |
| --- | --- | --- | --- |
| eCO₂ | ≤ 800 ppm | linear | ≥ 2,000 ppm |
| Temperature | 65–70 °F | −10 per °F outside | ≤ 55 or ≥ 80 °F |
| Humidity | 40–50% RH (placeholder) | −5 per % outside | ≤ 20% or ≥ 70% |

Bands: Great 90–100, Good 80–89, Fair 70–79, Poor < 70.
Flag readings during the first 20 minutes after CCS811 power-on (warm-up).

## Agent rules

- Every number the agent states must come from a tool result in the same turn;
  a backend check compares numbers in replies to tool results.
- Tools: get_current, get_summary(start, end), get_night_score(date), get_targets.
- Say so when data is missing; never estimate. Call CO₂ values "estimated (eCO₂)".
- Recommendations name the metric, value, target, and one concrete action. No medical advice.
- If the AI is down, dashboard and scoring keep working; chat shows an offline message.

## Hackathon rules to respect

- All code written during the event; public GitHub repo that stays public.
- List every AI tool used in the submission (including Claude / Claude Code) plus the
  model the agent runs on.
- API keys in environment variables, never committed.
- Demo: dashboard running → sanitizer spikes eCO₂ → score drops → ask agent why and show
  its tool calls → mention sensor-agnostic design (SCD41 is a one-driver upgrade).

Full requirements: SRS v2 (Hackathon Edition) doc —
https://claude.ai/code/artifact/f56e63c7-d1ca-4afb-8e42-485098ef1979
(Where it still says "embedded Rust on the MCU", the hybrid decision above wins.)
