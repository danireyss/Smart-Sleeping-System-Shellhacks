import type { NightReport, NightSummary, Targets } from './api'
import { METRICS, targetText, withUnit, type MetricKey } from './format'

/** Factual one-line summary of the night, from the report. No health claims. */
export function nightHeadline(night: NightReport, targets: Targets | null): { title: string; detail: string } {
  if (night.valid_readings === 0) {
    return { title: 'No valid readings during this session.', detail: 'Check that the sensor was connected and warmed up.' }
  }
  const lowest = night.lowest_metric
  const metric = METRICS.find((m) => m.key === lowest?.metric)
  if (!lowest || !metric) {
    return { title: 'All readings stayed in their target ranges.', detail: 'eCO₂ (estimated), temperature, and humidity were on target.' }
  }
  const stats = night[statsKey(metric.key)]
  const target = targets ? ` (${targetText(metric, targets).replace('Target ', 'target ')})` : ''
  return {
    title: `${metric.label} was the lowest-scoring reading.`,
    detail: stats
      ? `It averaged ${withUnit(metric, stats.avg)}${target} and was out of range for ${stats.minutes_out_of_range} of ${night.valid_minutes} minutes.`
      : '',
  }
}

const STATS_KEY = { eco2: 'eco2_ppm', temp: 'temp_f', humidity: 'humidity_pct' } as const

/** The NightReport field holding a metric's stats. */
export const statsKey = (key: MetricKey) => STATS_KEY[key]

/**
 * The calendar date a night belongs to, in local time, counted noon to noon:
 * going to bed at 12:30 AM on the 27th is still the night of the 26th.
 * Returned as "YYYY-MM-DD".
 */
export function nightDate(startedAt: string): string {
  const d = new Date(new Date(startedAt).getTime() - 12 * 60 * 60 * 1000)
  return dateKey(d)
}

export function dateKey(d: Date): string {
  const pad = (n: number) => String(n).padStart(2, '0')
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`
}

/**
 * One night per date for the calendar: if a date has several sessions (a nap,
 * a test), prefer full-length ones, then the longest.
 */
export function nightsByDate(nights: NightSummary[]): Map<string, NightSummary> {
  const byDate = new Map<string, NightSummary>()
  for (const n of nights) {
    const key = nightDate(n.started_at)
    const current = byDate.get(key)
    const better =
      !current ||
      (current.short_session && !n.short_session) ||
      (current.short_session === n.short_session && n.duration_minutes > current.duration_minutes)
    if (better) byDate.set(key, n)
  }
  return byDate
}
