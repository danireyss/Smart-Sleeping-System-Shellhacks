import type { Targets } from '@/lib/api'
import type { Metric } from '@/lib/format'

/** Thin bar over the metric's scale: target zone shaded, marker at the value. */
export function RangeBar({
  metric,
  targets,
  value,
  color,
}: {
  metric: Metric
  targets: Targets
  value: number | null
  color: string
}) {
  const { lo, hi } = metric.scale(targets)
  const { min, max } = metric.target(targets)
  const pct = (v: number) => `${(Math.min(Math.max((v - lo) / (hi - lo), 0), 1) * 100).toFixed(2)}%`
  const zoneStart = min === null ? lo : min

  return (
    <div className="relative h-1 w-full" aria-hidden>
      <div className="absolute inset-0 rounded-sm bg-line" />
      <div
        className="absolute inset-y-0 rounded-sm bg-great/80"
        style={{ left: pct(zoneStart), right: `calc(100% - ${pct(max)})` }}
      />
      {value !== null && (
        <div
          className="absolute top-1/2 size-2.5 -translate-x-1/2 -translate-y-1/2 rounded-full border-2 border-surface"
          style={{ left: pct(value), backgroundColor: color }}
        />
      )}
    </div>
  )
}
