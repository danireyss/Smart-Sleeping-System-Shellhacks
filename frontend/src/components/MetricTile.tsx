import { RangeBar } from '@/components/RangeBar'
import type { ScoredReading, Targets } from '@/lib/api'
import {
  BAND_TEXT,
  BAND_VAR,
  bandOf,
  offTarget,
  rangeStatus,
  targetText,
  type Metric,
} from '@/lib/format'
import { cn } from '@/lib/utils'

/** One live metric: value, target, range status, and range bar. Color only when out of range. */
export function MetricTile({
  metric,
  reading,
  targets,
  stale,
}: {
  metric: Metric
  reading: ScoredReading | null
  targets: Targets
  stale: boolean
}) {
  const value = reading ? metric.value(reading) : null
  const off = value !== null ? offTarget(metric, targets, value) : null
  const subScore = reading?.score?.[metric.key]
  // Out-of-range color follows the metric's sub-score band (amber, or coral if poor).
  const band = off && subScore !== undefined ? bandOf(subScore) : null
  const outColor = band && band !== 'great' ? band : off ? 'fair' : null

  return (
    <section
      className={cn(
        'flex flex-1 flex-col gap-5 rounded-2xl border p-6 transition-opacity',
        outColor === 'poor' && 'border-poor bg-poor/[0.07]',
        outColor === 'fair' && 'border-fair bg-fair/[0.07]',
        outColor === 'good' && 'border-good bg-good/[0.07]',
        !outColor && 'border-line bg-surface',
        stale && 'opacity-40',
      )}
      aria-label={metric.label}
    >
      <div className="flex items-center justify-between">
        <div className="flex items-baseline gap-1.5">
          <h3 className="text-sm font-semibold text-text">{metric.label}</h3>
          {metric.key === 'eco2' && (
            <span className="text-[10px] tracking-[0.5px] text-text-muted uppercase">estimated</span>
          )}
        </div>
        {value !== null && (
          <span
            className={cn(
              'text-xs font-medium tracking-[0.15px]',
              outColor ? BAND_TEXT[outColor] : 'text-great',
            )}
          >
            {rangeStatus(metric, targets, value)}
          </span>
        )}
      </div>
      <div className="flex flex-col gap-1">
        <p className="text-[34px] leading-[1.1] font-light tracking-[-1px] text-text">
          {value === null ? '—' : metric.format(value)}{' '}
          <span className="text-text-muted">{metric.unit}</span>
        </p>
        <p className="text-xs tracking-[0.15px] text-text-muted">{targetText(metric, targets)}</p>
      </div>
      <RangeBar
        metric={metric}
        targets={targets}
        value={value}
        color={outColor ? BAND_VAR[outColor] : 'var(--text)'}
      />
    </section>
  )
}
