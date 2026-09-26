import { Moon } from 'lucide-react'
import { useMemo } from 'react'

import { ChatTile } from '@/components/ChatTile'
import { MetricCharts } from '@/components/MetricCharts'
import { MetricTile } from '@/components/MetricTile'
import { useLive, useNow, useReadings, useSleepSession, useTargets } from '@/hooks/data'
import type { ScoredReading, Targets } from '@/lib/api'
import {
  BAND_BG,
  BAND_LABEL,
  BAND_TEXT,
  METRICS,
  STALE_AFTER_MIN,
  liveState,
  minutesSince,
  hoursMinutes,
  offTarget,
  tenths,
  timeOfDay,
  whole,
} from '@/lib/format'
import { cn } from '@/lib/utils'
import { useRouter } from '@/router'

const SIX_HOURS_MS = 6 * 60 * 60 * 1000

export function Home() {
  const { navigate } = useRouter()
  const { reading, unreachable } = useLive()
  const { targets, failed: targetsFailed } = useTargets()
  const now = useNow()
  const { session } = useSleepSession()

  // Chart window: last 6 hours, refetched each minute (and on each new reading).
  const minute = Math.floor(now / 60_000)
  const end = useMemo(() => new Date((minute + 1) * 60_000), [minute])
  const start = useMemo(() => new Date(end.getTime() - SIX_HOURS_MS), [end])
  const readings = useReadings(start, end, reading?.received_at)

  const state = liveState(reading, targets)
  const lastMin = reading ? minutesSince(reading.received_at, now) : 0
  const stale = !!reading && lastMin >= STALE_AFTER_MIN

  return (
    <main className="flex gap-8 p-10">
      <div className="flex min-w-0 flex-1 flex-col gap-8">
        {session && (
          <button
            type="button"
            onClick={() => navigate('/sleep')}
            className="flex items-center gap-3 rounded-xl border border-great/40 bg-great/[0.07] px-4 py-3 text-left text-sm text-text"
          >
            <Moon className="size-4 shrink-0 text-great" aria-hidden />
            <span>
              Sleep mode is on — started {timeOfDay(session.started_at)} on the device,{' '}
              {hoursMinutes((now - new Date(session.started_at).getTime()) / 60_000)} so far.
            </span>
          </button>
        )}

        {unreachable && (
          <p className="rounded-xl border border-poor/50 bg-poor/[0.07] px-4 py-3 text-sm text-text">
            Can't reach the sensor hub. Showing the last data received.
          </p>
        )}

        <section className="flex flex-col gap-4 rounded-2xl bg-surface p-8" aria-label="Current environment score">
          <div className="flex items-center justify-between">
            <h2 className="text-[11px] font-medium tracking-[1.5px] text-text-muted uppercase">
              Current environment score
            </h2>
            {state.kind === 'ok' && reading?.score && (
              <div className="flex items-center gap-1.5">
                <span className={cn('size-2 rounded-full', BAND_BG[reading.score.band])} />
                <span className={cn('text-sm font-semibold', BAND_TEXT[reading.score.band])}>
                  {BAND_LABEL[reading.score.band]}
                </span>
              </div>
            )}
          </div>
          <div className="flex items-baseline gap-6">
            <p className="text-[96px] leading-none font-thin tracking-[-3px] text-text">
              {state.kind === 'ok' && reading?.score ? tenths(reading.score.total) : '—'}
            </p>
            <div className="flex flex-col gap-1">
              <ScoreHeadline state={state} reading={reading ?? null} targets={targets} />
              {stale && <p className="text-sm text-fair">Last reading {lastMin} min ago</p>}
            </div>
          </div>
        </section>

        {targetsFailed && (
          <p className="rounded-xl border border-fair/50 bg-fair/[0.07] px-4 py-3 text-sm text-text">
            Couldn't load the scoring targets (GET /api/targets). The backend may be an older
            version: rebuild and restart it.
          </p>
        )}

        {targets && (
          <div className="flex gap-5">
            {METRICS.map((m) => (
              <MetricTile key={m.key} metric={m} reading={reading ?? null} targets={targets} stale={stale} />
            ))}
          </div>
        )}

        <section className="flex flex-col gap-5 rounded-2xl bg-surface p-6" aria-label="Last 6 hours">
          <h2 className="text-xs font-medium tracking-[0.15px] text-text-muted uppercase">Last 6 hours</h2>
          {targets && readings ? (
            readings.some((r) => r.score) ? (
              <MetricCharts readings={readings} targets={targets} start={start} end={end} variant="compact" />
            ) : (
              <p className="text-sm text-text-muted">No scored readings in the last 6 hours yet.</p>
            )
          ) : (
            <p className="text-sm text-text-muted">{targetsFailed ? 'Unavailable.' : 'Loading…'}</p>
          )}
        </section>
      </div>

      <ChatTile className="w-[440px] shrink-0" />
    </main>
  )
}

/** One sentence naming the biggest problem, or the current state. No health claims. */
function ScoreHeadline({
  state,
  reading,
  targets,
}: {
  state: ReturnType<typeof liveState>
  reading: ScoredReading | null
  targets: Targets | null
}) {
  if (state.kind === 'loading') return <p className="text-base text-text-muted">Loading…</p>
  if (state.kind === 'no-data') return <p className="text-base text-text">Waiting for the first reading…</p>
  if (state.kind === 'warming-up') {
    return (
      <>
        <p className="text-base font-medium text-text">Sensor warming up — scores in {state.minutesLeft} min</p>
        <p className="text-sm text-text-muted">The eCO₂ sensor needs about 20 minutes after power-on.</p>
      </>
    )
  }
  if (!reading?.score || !targets) return null

  // The biggest problem is the metric with the lowest sub-score that is out of range.
  const score = reading.score
  const off = METRICS.map((m) => {
    const v = m.value(reading)
    return { m, v, off: v === null ? null : offTarget(m, targets, v), sub: score[m.key] }
  })
    .filter((x) => x.off)
    .sort((a, b) => a.sub - b.sub)

  if (off.length === 0) {
    return (
      <>
        <p className="text-base font-medium text-text">All readings are in their target ranges.</p>
        <p className="text-sm text-text-muted">eCO₂ (estimated), temperature, and humidity are on target.</p>
      </>
    )
  }
  const worst = off[0]
  const gap = worst.m.key === 'eco2' ? whole(worst.off!.by) : tenths(worst.off!.by)
  const direction = worst.off!.dir === 'high' ? 'above' : 'below'
  const subject =
    worst.m.key === 'temp' ? 'Room is' : worst.m.key === 'eco2' ? 'eCO₂ (estimated) is' : 'Humidity is'
  const others = off.slice(1).map((x) => x.m.label)
  return (
    <>
      <p className="text-base font-medium text-text">
        {subject} {gap} {worst.m.unit} {direction} target.
      </p>
      <p className="text-sm text-text-muted">
        {others.length === 0 ? 'Other readings are in range.' : `Also out of range: ${others.join(', ')}.`}
      </p>
    </>
  )
}
