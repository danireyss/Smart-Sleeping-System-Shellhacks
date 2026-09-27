import { useMemo, useState } from 'react'

import { ChatTile } from '@/components/ChatTile'
import { MetricCharts } from '@/components/MetricCharts'
import { NightCalendar } from '@/components/NightCalendar'
import { NightScoreBlock } from '@/components/NightScore'
import { useNight, useNights, useReadings, useTargets } from '@/hooks/data'
import type { NightReport, NightSummary, Targets } from '@/lib/api'
import { nightDate, nightHeadline, statsKey } from '@/lib/night'
import { METRICS, longDate, timeOfDay, withUnit } from '@/lib/format'

const timeShort = (iso: string) => new Date(iso).toLocaleTimeString([], { hour: 'numeric', minute: '2-digit' })
import { cn } from '@/lib/utils'

/** ?night=<id> selects a past night; without it, the latest night is shown. */
function nightFromUrl(): number | null {
  const id = Number(new URLSearchParams(window.location.search).get('night'))
  return Number.isInteger(id) && id > 0 ? id : null
}

export function LastNight() {
  const [selectedId, setSelectedId] = useState<number | null>(nightFromUrl)
  const nights = useNights()
  const night = useNight(selectedId)
  const { targets } = useTargets()

  function select(id: number) {
    // The latest night is the default view, so it gets the plain URL.
    const isLatest = nights?.[0]?.session_id === id
    setSelectedId(isLatest ? null : id)
    window.history.replaceState(null, '', isLatest ? '/last-night' : `/last-night?night=${id}`)
  }

  if (nights === undefined || night === undefined) return null
  // The report comes from its own endpoint, so it still shows if the history can't load.
  if (night === null) {
    return (
      <main className="flex min-h-[calc(100vh-72px)] flex-col items-center justify-center gap-4 p-20 text-center">
        <h1 className="text-[22px] font-medium text-text">No finished sleep session yet</h1>
        <p className="text-[13px] text-text-muted">
          Tap Sleep on the device screen at bedtime and hold it to end sleep mode in the morning. Each night then
          shows up here, colored by its score.
        </p>
      </main>
    )
  }
  return (
    <Report
      night={night}
      targets={targets}
      nights={nights}
      isLatest={nights === null || night.session_id === nights[0]?.session_id}
      onSelect={select}
    />
  )
}

function Report({
  night,
  targets,
  nights,
  isLatest,
  onSelect,
}: {
  night: NightReport
  targets: Targets | null
  /** null when the history couldn't be loaded. */
  nights: NightSummary[] | null
  isLatest: boolean
  onSelect: (id: number) => void
}) {
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
              {isLatest ? 'Last night · ' : 'Night of '}
              {longDate(nightDateIso(night.started_at))}
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
              <MetricCharts
                readings={readings}
                targets={targets}
                start={start}
                end={end}
                variant="full"
                markers={{ sound: night.noise_events?.starts ?? [] }}
              />
            ) : (
              <p className="text-sm text-text-muted">No scored readings during this session.</p>
            )
          ) : (
            <p className="text-sm text-text-muted">Loading…</p>
          )}
        </section>

        <div className="grid grid-cols-3 gap-5">
          {METRICS.filter((m) => !m.optional || night[statsKey(m.key)]).map((m) => {
            const s = night[statsKey(m.key)]
            // Share of this metric's minutes inside the target range.
            const minutes = s?.minutes ?? night.valid_minutes
            const inRange =
              s && minutes > 0 ? Math.round(((minutes - s.minutes_out_of_range) / minutes) * 100) : null
            const noise = m.key === 'sound' ? night.noise_events : null
            return (
              <section key={m.key} className="flex flex-col gap-4 rounded-2xl bg-surface p-6">
                <h3 className="text-[13px] font-medium tracking-[0.2px] text-text-muted">
                  {/* Uppercase by hand: CSS would turn eCO₂ into ECO₂. */}
                  {m.key === 'eco2' ? 'eCO₂' : m.label.toUpperCase()}
                  {m.estimated && ' (ESTIMATED)'}
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
                    {noise && (
                      <p
                        className={cn('text-xs tracking-[0.15px]', noise.count === 0 ? 'text-text-muted' : 'text-fair')}
                        title={noise.starts.map(timeShort).join(', ')}
                      >
                        {noise.count === 0
                          ? `No noise events above ${noise.threshold_db} dB`
                          : `${noise.count} noise event${noise.count === 1 ? '' : 's'} above ${noise.threshold_db} dB (${noise.starts
                              .slice(0, 3)
                              .map(timeShort)
                              .join(', ')}${noise.count > 3 ? ', …' : ''})`}
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

      <div className="flex w-[440px] shrink-0 flex-col gap-8 self-start">
        {nights ? (
          <NightCalendar nights={nights} selectedId={night.session_id} onSelect={onSelect} />
        ) : (
          <p className="rounded-2xl border border-fair/50 bg-fair/[0.07] p-5 text-sm text-text">
            Couldn't load the night history (GET /api/nights). The backend may be an older version:
            rebuild and restart it.
          </p>
        )}
        <ChatTile />
      </div>
    </main>
  )
}

/** Noon of the night's calendar date, for formatting it as a date. */
function nightDateIso(startedAt: string): string {
  return `${nightDate(startedAt)}T12:00:00`
}
