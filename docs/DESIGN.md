# Frontend Design

Target: a laptop for the demo. Dark theme throughout.

## Home

- A big score with its band word, plus one sentence naming the biggest problem
  (e.g. "Room is 6.6 °F above target").
- Three metric tiles, each with:
  - the value
  - "Target X–Y"
  - a thin range bar (target zone shaded, marker at the current value)
  - color only when out of range
  - eCO₂ labeled "estimated"
- Three small stacked charts for the last 6 hours, with the target band shaded.
- A prominent "Start sleep mode" button.
- A chat tile on the right.

## Sleep mode

- Full-screen, true black, dim gray text.
- Shows only the three measurements and the current score, with a small band dot.
- No animation.
- Press-and-hold "End sleep mode" in a corner.
- Uses the Fullscreen API and Screen Wake Lock.

## Last night

Shown after sleep mode ends.

- Left side:
  - the nightly score and band
  - "Time in sleep mode: Xh Ym" (never "hours slept")
  - three stacked charts for the session
- Right side: the chat tile.

## Chat tile

- Under each agent reply, chips naming the tools it called
  ("Checked: current readings · last night's summary").
- Clicking a chip expands the numbers returned.
- A "✓ numbers verified" badge comes from the grounding check.

## States

| State | What the UI shows |
| --- | --- |
| Warm-up | Score as "—" with "Sensor warming up — scores in N min" |
| Stale sensor | Tiles dimmed, "Last reading N min ago" |
| No data | "Waiting for the first reading…" |
| AI offline | Chat input disabled, "Assistant offline — dashboard still live" |
| Incomplete night | Score replaced with "Incomplete night (N% of readings)" |
| Session under 1 hour | "Too short to be meaningful" note |

## Targets

Each tile's "Target X–Y" comes from the scoring targets; their sources are in
[REFERENCES.md](REFERENCES.md).

## Band colors (muted)

| Band | Color |
| --- | --- |
| Great | Teal |
| Good | Green |
| Fair | Amber |
| Poor | Coral |
