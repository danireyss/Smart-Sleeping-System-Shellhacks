import { useMemo } from 'react'

import { ChatTile } from '@/components/ChatTile'
import { MetricCharts } from '@/components/MetricCharts'
import { NightScoreBlock } from '@/components/NightScore'
import { useLatestNight, useReadings, useTargets } from '@/hooks/data'
import type { NightReport, Targets } from '@/lib/api'
import { nightHeadline, statsKey } from '@/lib/night'
import { METRICS, longDate, timeOfDay, withUnit } from '@/lib/format'
import { cn } from '@/lib/utils'
import { useRouter } from '@/router'

export function LastNight() {
  const night = useLatestNight()
  const { targets } = useTargets()
  const { navigate } = useRouter()

  if (night === undefined) return null
  if (night === null) {
    return (
      <main className="flex min-h-[calc(100vh-72px)] flex-col items-center justify-center gap-4 p-20 text-center">
        <h1 className="text-[22px] font-medium text-text">No finished sleep session yet</h1>
        <p className="text-[13px] text-text-muted">Start sleep mode at bedtime and end it in the morning to see a report here.</p>
        <button
          type="button"
          onClick={() => navigate('/sleep')}
          className="rounded-full bg-great px-8 py-3 text-sm font-semibold text-bg hover:opacity-90"
        >
          Go to sleep mode
        </button>
      </main>
    )
  }
  return <Report night={night} targets={targets} />
}

function Report({ night, targets }: { night: NightReport; targets: Targets | null }) {
  const start = useMemo(() => new Date(night.started_at), [night.started_at])
  const end = useMemo(() => new Date(night.ended_at), [night.ended_at])
  const readings = useReadings(start, end)
  const { title, detail } = nightHeadline(night, targets)

  return (
    <main className="flex gap-8 p-10">
      <div className="flex min-w-0 flex-1 flex-col gap-8">
        <section className="flex items-center justify-between gap-8 rounded-2xl bg-surface p-8">
          <div className="flex min-w-0 flex-col gap-2">
            <p className="text-[11px] font-medium tracking-[1.5px] text-text-muted uppercase">
              Last night · {longDate(night.ended_at)}
            </p>
            <h1 className="text-lg text-text">{title}</h1>
            {detail && <p className="text-[15px] leading-[1.6] text-text-muted">{detail}</p>}
          </div>
          <NightScoreBlock night={night} align="right" />
        </section>

        <section className="flex flex-col gap-6 rounded-2xl bg-surface p-8" aria-label="Session charts">
          <h2 className="text-xs font-medium tracking-[0.15px] text-text-muted uppercase">
            Sleep mode · {timeOfDay(night.started_at)} – {timeOfDay(night.ended_at)}
          </h2>
          {targets && readings ? (
            readings.some((r) => r.score) ? (
              <MetricCharts readings={readings} targets={targets} start={start} end={end} variant="full" />
            ) : (
              <p className="text-sm text-text-muted">No scored readings during this session.</p>
            )
          ) : (
            <p className="text-sm text-text-muted">Loading…</p>
          )}
        </section>

        <div className="flex gap-5">
          {METRICS.map((m) => {
            const s = night[statsKey(m.key)]
            // Share of valid minutes inside the target range.
            const inRange =
              s && night.valid_minutes > 0
                ? Math.round(((night.valid_minutes - s.minutes_out_of_range) / night.valid_minutes) * 100)
                : null
            return (
              <section key={m.key} className="flex flex-1 flex-col gap-4 rounded-2xl bg-surface p-6">
                <h3 className="text-[13px] font-medium tracking-[0.2px] text-text-muted">
                  {/* Uppercase by hand: CSS would turn eCO₂ into ECO₂. */}
                  {m.key === 'eco2' ? 'eCO₂ (ESTIMATED)' : m.label.toUpperCase()}
                </h3>
                {s ? (
                  <>
                    <div className="flex items-start justify-between">
                      <div className="flex flex-col gap-0.5">
                        <p className="text-[11px] tracking-[0.2px] text-text-dim uppercase">Average</p>
                        <p className="text-2xl leading-[1.1] font-light tracking-[-0.5px] text-text">
                          {withUnit(m, s.avg)}
                        </p>
                      </div>
                      <div className="flex flex-col items-end gap-0.5">
                        <p className="text-[11px] tracking-[0.2px] text-text-dim uppercase">Min / Max</p>
                        <p className="text-[15px] leading-[1.6] text-text-muted">
                          {m.format(s.min)} / {m.format(s.max)}
                        </p>
                      </div>
                    </div>
                    {inRange !== null && (
                      <p
                        className={cn(
                          'text-xs font-medium tracking-[0.15px]',
                          inRange === 100 ? 'text-great' : 'text-fair',
                        )}
                      >
                        {inRange}% of the time in target range
                      </p>
                    )}
                  </>
                ) : (
                  <p className="text-sm text-text-muted">No valid readings.</p>
                )}
              </section>
            )
          })}
        </div>
      </div>

      <ChatTile className="w-[440px] shrink-0 self-start" />
    </main>
  )
}
