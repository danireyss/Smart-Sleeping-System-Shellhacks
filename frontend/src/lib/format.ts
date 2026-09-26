// Display rules. Numbers are shown exactly as the API rounds them: eCO₂ and
// TVOC as whole numbers, temperature, humidity, and scores to 1 decimal. JSON
// drops trailing zeros (77.0 arrives as 77), so tenths are re-padded here.
import type { Band, ScoredReading, Targets } from './api'

export const tenths = (v: number) => v.toFixed(1)
export const whole = (v: number) => Math.round(v).toString()

export type MetricKey = 'eco2' | 'temp' | 'humidity'

export interface Metric {
  key: MetricKey
  label: string
  short: string
  unit: string
  /** Suffix after the number ("ppm" with a space, "%" without in compact spots). */
  format: (v: number) => string
  value: (r: ScoredReading) => number | null
  /** Target range for display and the range bar; min is null for eCO₂ (≤ max). */
  target: (t: Targets) => { min: number | null; max: number }
  /** Scale for range bars and chart axes: where the sub-score reaches 0. */
  scale: (t: Targets) => { lo: number; hi: number }
}

export const METRICS: Metric[] = [
  {
    key: 'eco2',
    label: 'eCO₂',
    short: 'eCO₂',
    unit: 'ppm',
    format: whole,
    value: (r) => r.eco2_ppm,
    target: (t) => ({ min: null, max: t.eco2_ppm.full_points_at_or_below }),
    scale: (t) => ({ lo: 400, hi: t.eco2_ppm.zero_points_at_or_above }),
  },
  {
    key: 'temp',
    label: 'Temperature',
    short: 'Temp',
    unit: '°F',
    format: tenths,
    value: (r) => r.temp_f,
    target: (t) => ({ min: t.temp_f.target_min, max: t.temp_f.target_max }),
    scale: (t) => ({ lo: t.temp_f.zero_points_at_or_below, hi: t.temp_f.zero_points_at_or_above }),
  },
  {
    key: 'humidity',
    label: 'Humidity',
    short: 'Humidity',
    unit: '%',
    format: tenths,
    value: (r) => r.humidity_pct,
    target: (t) => ({ min: t.humidity_pct.target_min, max: t.humidity_pct.target_max }),
    scale: (t) => ({
      lo: t.humidity_pct.zero_points_at_or_below,
      hi: t.humidity_pct.zero_points_at_or_above,
    }),
  },
]

export const withUnit = (m: Metric, v: number) => `${m.format(v)} ${m.unit}`

export function targetText(m: Metric, t: Targets): string {
  const { min, max } = m.target(t)
  const f = (v: number) => (m.key === 'eco2' ? whole(v) : String(v))
  return min === null ? `Target ≤ ${f(max)} ${m.unit}` : `Target ${f(min)}–${f(max)} ${m.unit}`
}

/** How far a value sits outside its target: null when in range. */
export function offTarget(m: Metric, t: Targets, v: number): { by: number; dir: 'high' | 'low' } | null {
  const { min, max } = m.target(t)
  // Compare at display precision so a shown 70.0 is never "0.0 °F high".
  const shown = Number(m.format(v))
  if (shown > max) return { by: shown - max, dir: 'high' }
  if (min !== null && shown < min) return { by: min - shown, dir: 'low' }
  return null
}

/** Tile badge: "In range" or "6.6 °F high". */
export function rangeStatus(m: Metric, t: Targets, v: number): string {
  const off = offTarget(m, t, v)
  if (!off) return 'In range'
  const gap = m.key === 'eco2' ? whole(off.by) : tenths(off.by)
  return `${gap} ${m.unit} ${off.dir}`
}

export const BAND_LABEL: Record<Band, string> = {
  great: 'Great',
  good: 'Good',
  fair: 'Fair',
  poor: 'Poor',
}

/** Tailwind text/background classes per band (colors from DESIGN.md). */
export const BAND_TEXT: Record<Band, string> = {
  great: 'text-great',
  good: 'text-good',
  fair: 'text-fair',
  poor: 'text-poor',
}
export const BAND_BG: Record<Band, string> = {
  great: 'bg-great',
  good: 'bg-good',
  fair: 'bg-fair',
  poor: 'bg-poor',
}
export const BAND_VAR: Record<Band, string> = {
  great: 'var(--great)',
  good: 'var(--good)',
  fair: 'var(--fair)',
  poor: 'var(--poor)',
}

/** Band for a 0–100 sub-score, using the same thresholds as the backend. */
export function bandOf(score: number): Band {
  if (score >= 90) return 'great'
  if (score >= 80) return 'good'
  if (score >= 70) return 'fair'
  return 'poor'
}

/** "8h 50m" (the same form the agent's time_in_sleep_mode uses). */
export function hoursMinutes(totalMinutes: number): string {
  const m = Math.max(0, Math.floor(totalMinutes))
  return `${Math.floor(m / 60)}h ${m % 60}m`
}

export const minutesSince = (iso: string, now = Date.now()) =>
  Math.max(0, Math.floor((now - new Date(iso).getTime()) / 60_000))

/** A reading older than this counts as stale (production interval is 60 s). */
export const STALE_AFTER_MIN = 3

export type LiveState =
  | { kind: 'loading' }
  | { kind: 'no-data' }
  | { kind: 'warming-up'; minutesLeft: number }
  | { kind: 'ok' }

export function liveState(r: ScoredReading | null | undefined, t: Targets | null): LiveState {
  if (r === undefined) return { kind: 'loading' }
  if (r === null) return { kind: 'no-data' }
  if (r.flags.includes('warm_up')) {
    const warmUpSecs = (t?.sensor_warm_up_minutes ?? 20) * 60
    return { kind: 'warming-up', minutesLeft: Math.max(1, Math.ceil((warmUpSecs - r.uptime_s) / 60)) }
  }
  return { kind: 'ok' }
}

export const timeOfDay = (iso: string) =>
  new Date(iso).toLocaleTimeString([], { hour: 'numeric', minute: '2-digit' })

export const longDate = (iso: string) =>
  new Date(iso).toLocaleDateString([], { weekday: 'long', month: 'short', day: 'numeric' })
