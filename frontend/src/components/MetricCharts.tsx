import { Line, LineChart, ReferenceArea, ReferenceLine, XAxis, YAxis } from 'recharts'

import { ChartContainer, ChartTooltip, type ChartConfig } from '@/components/ui/chart'
import type { ScoredReading, Targets } from '@/lib/api'
import { BAND_VAR, bandOf, offTarget, visibleMetrics, withUnit, type Metric, type MetricKey } from '@/lib/format'

interface Point {
  t: number
  v: number | null
}

/**
 * Stacked charts, one per metric (eCO₂, temperature, humidity, plus light and
 * sound when the webcam provided them), each with its target band shaded.
 */
export function MetricCharts({
  readings,
  targets,
  start,
  end,
  variant,
  markers,
}: {
  readings: ScoredReading[]
  targets: Targets
  start: Date
  end: Date
  /** "compact": home sparklines with the latest value; "full": larger with a time axis. */
  variant: 'compact' | 'full'
  /** Vertical marker lines per metric (e.g. noise events on the sound chart), as ISO times. */
  markers?: Partial<Record<MetricKey, string[]>>
}) {
  // Flagged readings (warm-up, bad values) aren't plotted, matching the scores.
  const valid = readings.filter((r) => r.score !== null)
  const metrics = visibleMetrics(valid)
  return (
    <div className="flex flex-col gap-4">
      {metrics.map((m, i) => (
        <MetricChart
          key={m.key}
          metric={m}
          points={valid.map((r) => ({ t: new Date(r.received_at).getTime(), v: m.value(r) }))}
          lastSubScore={valid.at(-1)?.score?.[m.key] ?? null}
          targets={targets}
          start={start}
          end={end}
          variant={variant}
          showAxis={variant === 'full' && i === metrics.length - 1}
          markers={(markers?.[m.key] ?? []).map((t) => new Date(t).getTime())}
        />
      ))}
    </div>
  )
}

function MetricChart({
  metric,
  points,
  lastSubScore,
  targets,
  start,
  end,
  variant,
  showAxis,
  markers,
}: {
  metric: Metric
  points: Point[]
  /** Sub-score of the latest point, for the same out-of-range color as the tile. */
  lastSubScore: number | null
  targets: Targets
  start: Date
  end: Date
  variant: 'compact' | 'full'
  showAxis: boolean
  markers: number[]
}) {
  const values = points.map((p) => p.v).filter((v): v is number => v !== null)
  const last = values.at(-1) ?? null
  const { min, max } = metric.target(targets)
  const zoneLo = min ?? metric.scale(targets).lo

  // Y range covers the data and the target band, padded a little.
  const lo = Math.min(zoneLo, ...values)
  const hi = Math.max(max, ...values)
  const pad = (hi - lo) * 0.15 || 1

  // Line color: teal when the latest value is in range; otherwise the band color
  // of its sub-score, matching the metric tile.
  const outOfRange = last !== null && offTarget(metric, targets, last)
  const outBand = lastSubScore === null ? 'fair' : bandOf(lastSubScore)
  const color = outOfRange ? BAND_VAR[outBand === 'great' ? 'fair' : outBand] : 'var(--great)'

  const config = { v: { label: metric.short, color } } satisfies ChartConfig
  const height = variant === 'compact' ? 36 : showAxis ? 128 : 104

  return (
    <div className="flex items-center gap-4">
      <p className="w-20 shrink-0 text-xs tracking-[0.15px] text-text-muted">{metric.short}</p>
      <ChartContainer config={config} className="aspect-auto w-full min-w-0 flex-1" style={{ height }}>
        <LineChart data={points} margin={{ top: 4, right: 4, bottom: 0, left: 4 }}>
          <XAxis
            dataKey="t"
            type="number"
            scale="time"
            domain={[start.getTime(), end.getTime()]}
            ticks={hourTicks(start, end)}
            hide={!showAxis}
            tickFormatter={(t: number) => new Date(t).toLocaleTimeString([], { hour: 'numeric' })}
            tick={{ fill: 'var(--text-muted)', fontSize: 11 }}
            tickLine={false}
            axisLine={false}
          />
          <YAxis hide domain={[lo - pad, hi + pad]} />
          <ReferenceArea y1={zoneLo} y2={max} fill="var(--great)" fillOpacity={0.08} ifOverflow="extendDomain" />
          {markers.map((t) => (
            <ReferenceLine key={t} x={t} stroke="var(--poor)" strokeDasharray="3 3" strokeOpacity={0.7} />
          ))}
          <ChartTooltip
            cursor={{ stroke: 'var(--line)' }}
            content={({ active, payload }) => {
              const p = payload?.[0]?.payload as Point | undefined
              if (!active || !p || p.v === null) return null
              return (
                <div className="rounded-md border border-line bg-surface px-2.5 py-1.5 text-xs">
                  <p className="text-text">{withUnit(metric, p.v)}</p>
                  <p className="text-text-muted">
                    {new Date(p.t).toLocaleTimeString([], { hour: 'numeric', minute: '2-digit' })}
                  </p>
                </div>
              )
            }}
          />
          <Line
            dataKey="v"
            type="monotone"
            stroke="var(--color-v)"
            strokeWidth={1.5}
            // Sparse data (e.g. an incomplete night) would otherwise draw nothing.
            dot={points.length < 30 ? { r: 1.5, fill: 'var(--color-v)', strokeWidth: 0 } : false}
            connectNulls={false}
            isAnimationActive={false}
          />
        </LineChart>
      </ChartContainer>
      {variant === 'compact' && (
        <p className="w-[72px] shrink-0 text-right text-xs tracking-[0.15px] text-text">
          {last === null ? '—' : withUnit(metric, last)}
        </p>
      )}
    </div>
  )
}

/** Ticks on the hour (every 2 h for long ranges), so labels never repeat. */
function hourTicks(start: Date, end: Date): number[] {
  const hour = 60 * 60 * 1000
  const step = end.getTime() - start.getTime() > 10 * hour ? 2 * hour : hour
  const first = new Date(start)
  first.setMinutes(0, 0, 0)
  const ticks: number[] = []
  for (let t = first.getTime() + hour; t <= end.getTime(); t += hour) {
    if ((t - first.getTime() - hour) % step === 0) ticks.push(t)
  }
  return ticks
}
