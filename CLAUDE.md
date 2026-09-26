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
- **Bridge verified (Sat Sep 26):** App Lab app `sensor-test` runs `firmware/sensor_bridge.ino`,
  which calls `Bridge.notify("reading", ...)`; `router-test/` (Rust, `rmpv`) registers `reading`
  on the router socket and receives the readings. No sudo needed. Next: backend (see build order).

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
- **Frontend:** Vite + React + TypeScript + Tailwind v4 + shadcn/ui + zod + Recharts in
  `frontend/`. Built on a dev machine (`npm run build`); `frontend/dist` is committed and
  embedded in the backend binary (rust-embed), so the board needs no Node and the UI
  works offline. amicro animations only sparingly, never in sleep mode.
- **Sleep sessions replace the fixed sleep window.** No bedtime/wake-time setting: the
  user starts and ends a session (`POST /api/sleep/start` / `end`), and a night is the
  span of one ended session. At most one session is open at a time.
- **Rounding at serialization.** eCO₂ and TVOC as integers; temperature, humidity, and
  all scores to 1 decimal, in every API response and SSE event, so the dashboard, the
  agent, and the grounding check see the same numbers. Flags, sub-scores, and stats are
  computed from the rounded reading (`Reading::rounded`), so a shown 79.0 °F scores exactly
  10.0. Sub-scores are rounded first; the total and band come from the rounded values.

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
socket at `/var/run/arduino-router.sock` (confirmed with `sudo ss -xlp | grep -i router`).
- Register a method: `[0, msgid, "$/register", ["reading"]]` → `[1, msgid, null, true]`
- Requests: `[0, msgid, method, params]`, responses: `[1, msgid, error, result]`,
  notifications: `[2, method, params]`.
- Registrations drop when the client disconnects. `$/unregister`, `$/reset` also exist.

## Bridge (verified)

- MCU sketch (`firmware/sensor_bridge.ino`, App Lab app `sensor-test`) keeps the latest
  CCS811 values and every 10 s (60 s in production) reads the DHT11, sets compensation,
  and calls `Bridge.notify("reading", (int)eco2, (int)tvoc, temp_f, humidity, uptime_s)`.
  temp_f/humidity are NaN if the DHT11 read fails; uptime_s = millis()/1000. Param order is
  documented in the sketch and in `backend/src/adapters/bridge.rs` — keep them in sync.
- `router-test/` connects to the socket, sends `$/register` for `reading`, and prints
  notifications (params `[eco2, tvoc, temp_f, humidity, uptime_s]`). It replies `[1, id, nil, true]`
  to requests, so `Bridge.call` would also work. Run: `cargo run -p router-test`.
- Board runs ~78 °F on the DHT11; check for board heat before the overnight run.

## Backend layout (target)

```
(repo root = Cargo workspace: backend/, router-test/)
└── backend/src/
    ├── main.rs          # wiring: repos → services → router, spawn tasks
    ├── config.rs        # targets, intervals, AI base URL/key/model from env
    ├── domain/          # Reading + validation flags, scoring math (unit-tested, no I/O)
    ├── controllers/     # readings.rs, stream.rs (SSE), chat.rs
    ├── services/        # ingest_service, reading_service, agent_service
    ├── repositories/    # ReadingRepository trait + sqlite_repo.rs
    └── adapters/        # bridge.rs (router client), llm_client.rs
```

Flow: bridge.rs → ingest_service (validate, store, score) → Tokio broadcast channel →
SSE clients. Agent tools call reading_service (same path as the dashboard).

API: `GET /api/current`, `GET /api/readings?start=&end=`, `GET /api/summary?start=&end=`,
`GET /api/stream` (SSE), `POST /api/sleep/start`, `POST /api/sleep/end`,
`GET /api/sleep/current`, `GET /api/night/latest`, `POST /api/chat`, `GET /` (frontend).

Implemented (server on `BIND_ADDR`, default `0.0.0.0:8080`):
- `/api/current`: latest reading + `flags` + `score` {eco2, temp, humidity, total, band}
  (`score` null when flagged); 404 `{"error"}` if no readings.
- `/api/summary?start=…Z&end=…Z`: RFC 3339, `[start, end)`. Counts, `valid_minutes`, and
  per metric {avg, min, max, minutes_out_of_range} over valid readings, plus score
  {avg, min, max, band}. Out of range = outside the 100-point target, counted as distinct
  minutes. Use `Z` or URL-encode `+` offsets. 400 `{"error"}` on bad params.
- `/api/readings?start=…Z&end=…Z`: every reading in `[start, end)`, oldest first, same
  shape as `/api/current` (for charts). Same params/errors as `/api/summary`.
- `/api/stream`: SSE `event: reading`, data = same JSON as `/api/current`; 15 s keep-alives.
  Ingest publishes only readings that were saved.
- `POST /api/sleep/start`: 201 + session `{id, started_at, ended_at: null}`; 409 if one is
  open. `POST /api/sleep/end`: 200 + closed session; 409 if none is open.
  `GET /api/sleep/current`: the open session or `null`. Times are server UTC.
- `/api/night/latest`: report for the most recently ended session: started/ended_at,
  `duration_minutes`, `short_session` (< 1 h), `score` + `band`, `completeness_pct` +
  `incomplete`, reading counts, per-metric {avg, min, max, avg_score,
  minutes_out_of_range}, `lowest_metric` {metric, avg_score} (null if all average 100).
  404 if no session has ended.
- Summary stats also include each metric's `avg_score` (average 0–100 sub-score).

Crates: tokio, axum, sqlx or rusqlite, serde, rmpv/rmp-serde, async-openai, chrono, tracing.

Build order: bridge + ingest printing readings → storage → scoring with tests →
API + SSE → frontend → agent.

Builds happen on the board (aarch64 Debian, slow first build). Use debug builds while
developing, `--release` for the overnight run and demo.

## Scoring

Per-minute score = average of three 0–100 sub-scores; nightly score = average of minute
scores in the sleep session (user starts/ends it). Exclude flagged/missing minutes;
a night with <60% valid minutes is "incomplete".

| Metric | 100 points | Outside | 0 points at |
| --- | --- | --- | --- |
| eCO₂ | ≤ 800 ppm | linear | ≥ 2,000 ppm |
| Temperature | 65–70 °F | −10 per °F outside | ≤ 55 or ≥ 80 °F |
| Humidity | 40–60% RH | −5 per % outside | ≤ 20% or ≥ 80% |

Bands: Great 90–100, Good 80–89, Fair 70–79, Poor < 70 (thresholds 90/80/70, so 89.9 is Good).
A reading's score is its minute score. Nightly score = average of the scored (unflagged)
readings in `[started_at, ended_at)` of the session (equals averaging minutes, since the
interval is constant). "Incomplete" = distinct minutes with ≥ 1 valid reading ÷ session
length in minutes (rounded, at least 1) < 60%, so it works at both the 10 s dev and 60 s
production intervals. Sessions under 1 hour are marked `short_session`.
Flag (exclude) readings when: uptime_s < 1200 (CCS811 warm-up), eco2 == 0, eCO₂ outside
400–8192 ppm, temp missing or outside 32–120 °F, RH missing or outside 0–100%.
Readings are timestamped in UTC by the backend on receipt (the MCU has no clock).

## Agent rules

- Every number the agent states must come from a tool result in the same turn;
  a backend check compares numbers in replies to tool results.
- Tools: get_current, get_summary(start, end), get_night_latest, get_targets. They call
  services in-process (never repositories). get_current adds `minutes_since_reading`;
  get_night_latest adds `time_in_sleep_mode` ("8h 50m") so those numbers are grounded.
- Say so when data is missing; never estimate. Call CO₂ values "estimated (eCO₂)".
- Recommendations name the metric, value, target, and one concrete action. No medical advice.
- If the AI is down, dashboard and scoring keep working; chat shows an offline message.

Implemented in `services/agent_service.rs` (loop, tools, system prompt),
`adapters/llm_client.rs` (async-openai `byot` streaming, 20 s start/idle timeouts), and
`domain/grounding.rs`:
- Config: `LLM_BASE_URL` (default Groq `https://api.groq.com/openai/v1`), `LLM_API_KEY`,
  `LLM_MODEL`. Missing key/model = assistant offline. The backend loads a gitignored `.env`
  (see `.env.example`); never commit the key.
- `POST /api/chat` `{"message", "history": [{"role": "user"|"assistant", "content"}]}`
  (message ≤ 2000 chars, ≤ 20 history turns) → SSE events: `tool_call` {name, arguments,
  result}, `token` {text}, `done` {grounding: {verified, checked, unmatched}}, or `offline`
  {message} if the model is missing or unreachable (no `done` after it).
- Up to 4 tool rounds per turn, then the model must answer in text.
- Grounding: every unsigned number in the reply (commas stripped; digits after letters
  like eCO2 ignored) must equal a number in this turn's tool results (JSON text, so
  numbers inside strings count) or in the user's current message ("is 72 °F too hot?").
  Earlier turns don't count.
- `CHAT_TOKEN` (optional): when set, `/api/chat` requires `Authorization: Bearer <token>`
  (401 otherwise). Set it before exposing the board (e.g. Cloudflare Tunnel), since chat
  spends the Groq quota. Unset = open, fine on the local hotspot.

## Hackathon rules to respect

- All code written during the event; public GitHub repo that stays public.
- List every AI tool used in the submission (including Claude / Claude Code) plus the
  model the agent runs on.
- API keys in environment variables, never committed.
- Demo: dashboard running → sanitizer spikes eCO₂ → score drops → ask agent why and show
  its tool calls → mention sensor-agnostic design (SCD41 is a one-driver upgrade).

Frontend design (screens, states, band colors): [docs/DESIGN.md](docs/DESIGN.md), Figma
file `u2jXvMjUHIPUtmDqzfo17A` (frames home-live-view, sleep-mode-active,
sleep-morning-summary, history-morning-report). Implementation notes: `frontend/README.md`.
UI rules: numbers shown exactly at API precision (eCO₂ whole, others 1 decimal); tile
badges show range status ("In range" / "6.6 °F high"); sleep mode is true black with no
motion; no health claims in any copy. Routes `/`, `/sleep`, `/last-night` (the backend
serves index.html for them). `GET /api/targets` gives the UI the same targets as the agent.
**After changing the frontend, run `npm run build` in `frontend/` and commit `dist/`.**
Sources for the scoring targets: [docs/REFERENCES.md](docs/REFERENCES.md) (the agent cites
their author-year labels via get_targets).

Full requirements: SRS v2 (Hackathon Edition) doc —
https://claude.ai/code/artifact/f56e63c7-d1ca-4afb-8e42-485098ef1979
(Where it still says "embedded Rust on the MCU" or describes a fixed bedtime/wake sleep
window, the decisions above win.)
