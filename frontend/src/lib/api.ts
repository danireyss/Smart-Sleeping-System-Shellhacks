// Typed access to the backend API. Every response is validated with zod, so a
// shape mismatch fails loudly instead of rendering wrong numbers. Numbers are
// shown exactly as the API rounds them (see lib/format.ts).
import { z } from 'zod'

export const Band = z.enum(['great', 'good', 'fair', 'poor'])
export type Band = z.infer<typeof Band>

export const MinuteScore = z.object({
  eco2: z.number(),
  temp: z.number(),
  humidity: z.number(),
  total: z.number(),
  band: Band,
})
export type MinuteScore = z.infer<typeof MinuteScore>

export const ScoredReading = z.object({
  received_at: z.string(),
  eco2_ppm: z.number(),
  tvoc_ppb: z.number(),
  temp_f: z.number().nullable(),
  humidity_pct: z.number().nullable(),
  uptime_s: z.number(),
  flags: z.array(z.string()),
  score: MinuteScore.nullable(),
})
export type ScoredReading = z.infer<typeof ScoredReading>

export const MetricStats = z.object({
  avg: z.number(),
  min: z.number(),
  max: z.number(),
  avg_score: z.number(),
  minutes_out_of_range: z.number(),
})
export type MetricStats = z.infer<typeof MetricStats>

export const SleepSession = z.object({
  id: z.number(),
  started_at: z.string(),
  ended_at: z.string().nullable(),
})
export type SleepSession = z.infer<typeof SleepSession>

export const NightReport = z.object({
  session_id: z.number(),
  started_at: z.string(),
  ended_at: z.string(),
  duration_minutes: z.number(),
  short_session: z.boolean(),
  score: z.number().nullable(),
  band: Band.nullable(),
  completeness_pct: z.number(),
  incomplete: z.boolean(),
  readings: z.number(),
  valid_readings: z.number(),
  valid_minutes: z.number(),
  eco2_ppm: MetricStats.nullable(),
  temp_f: MetricStats.nullable(),
  humidity_pct: MetricStats.nullable(),
  lowest_metric: z.object({ metric: z.string(), avg_score: z.number() }).nullable(),
})
export type NightReport = z.infer<typeof NightReport>

const Range = z.object({ target_min: z.number(), target_max: z.number() })
export const Targets = z.object({
  eco2_ppm: z.object({ full_points_at_or_below: z.number(), zero_points_at_or_above: z.number() }),
  temp_f: Range.extend({ zero_points_at_or_below: z.number(), zero_points_at_or_above: z.number() }),
  humidity_pct: Range.extend({ zero_points_at_or_below: z.number(), zero_points_at_or_above: z.number() }),
  sensor_warm_up_minutes: z.number(),
})
export type Targets = z.infer<typeof Targets>

/** Thrown for 404s so callers can show "no data" states. */
export class NotFound extends Error {}

async function getJson<T>(path: string, schema: z.ZodType<T>): Promise<T> {
  const res = await fetch(path)
  if (res.status === 404) throw new NotFound(path)
  if (!res.ok) throw new Error(`${path}: HTTP ${res.status}`)
  return schema.parse(await res.json())
}

async function postJson<T>(path: string, schema: z.ZodType<T>): Promise<T> {
  const res = await fetch(path, { method: 'POST' })
  if (!res.ok) throw new Error(`${path}: HTTP ${res.status}`)
  return schema.parse(await res.json())
}

/** Latest reading, or null when there are no readings yet. */
export async function getCurrent(): Promise<ScoredReading | null> {
  try {
    return await getJson('/api/current', ScoredReading)
  } catch (e) {
    if (e instanceof NotFound) return null
    throw e
  }
}

export function getReadings(start: Date, end: Date): Promise<ScoredReading[]> {
  const q = new URLSearchParams({ start: start.toISOString(), end: end.toISOString() })
  return getJson(`/api/readings?${q}`, z.array(ScoredReading))
}

export const getTargets = () => getJson('/api/targets', Targets)
export const getSleepCurrent = () => getJson('/api/sleep/current', SleepSession.nullable())
export const startSleep = () => postJson('/api/sleep/start', SleepSession)
export const endSleep = () => postJson('/api/sleep/end', SleepSession)

/** Report for the last ended session, or null if none has ended. */
export async function getLatestNight(): Promise<NightReport | null> {
  try {
    return await getJson('/api/night/latest', NightReport)
  } catch (e) {
    if (e instanceof NotFound) return null
    throw e
  }
}
