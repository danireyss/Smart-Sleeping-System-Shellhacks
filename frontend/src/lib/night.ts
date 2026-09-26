import type { NightReport, Targets } from './api'
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
