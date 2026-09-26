import { Moon, Sun } from 'lucide-react'
import { useEffect, useState } from 'react'

import { NightScoreBlock } from '@/components/NightScore'
import { useLive, useNow, useSleepSession, useTargets } from '@/hooks/data'
import { getLatestNight, type NightReport, type SleepSession, type Targets } from '@/lib/api'
import {
  BAND_BG,
  BAND_LABEL,
  METRICS,
  STALE_AFTER_MIN,
  hoursMinutes,
  liveState,
  longDate,
  minutesSince,
  tenths,
  timeOfDay,
  withUnit,
} from '@/lib/format'
import { nightHeadline, statsKey } from '@/lib/night'
import { cn } from '@/lib/utils'
import { useRouter } from '@/router'

/**
 * Sleep mode is started and ended on the device's touch screen. This page shows
 * its state live: the session in progress, how to start one, or, once a session
 * ends while the page is open, the morning summary.
 */
export function Sleep() {
  const { session, justEnded } = useSleepSession()
  const [night, setNight] = useState<NightReport | null | undefined>(undefined)

  useEffect(() => {
    if (justEnded) getLatestNight().then(setNight, () => setNight(null))
  }, [justEnded])

  if (session === undefined) return null
  if (session) return <InProgress session={session} />
  if (justEnded && night !== undefined) return <MorningSummary night={night} />
  return <Off />
}

function Off() {
  const { navigate } = useRouter()
  return (
    <main className="flex min-h-[calc(100vh-72px)] flex-col items-center justify-center gap-8 p-20 text-center">
      <div className="flex size-[120px] items-center justify-center rounded-full bg-surface-2">
        <Moon className="size-12 text-text-muted" aria-hidden />
      </div>
      <div className="flex max-w-md flex-col gap-2">
        <h1 className="text-[22px] font-medium tracking-[-0.3px] text-text">Sleep mode is off</h1>
        <p className="text-[13px] leading-[1.65] text-text-muted">
          Tap <span className="text-text">Sleep</span> on the device screen at bedtime. The screen dims,
          and this page follows along. Hold the button to end sleep mode in the morning.
        </p>
      </div>
      <button
        type="button"
        onClick={() => navigate('/last-night')}
        className="rounded-full border border-line px-8 py-3 text-sm font-semibold text-text-muted hover:text-text"
      >
        View last night
      </button>
    </main>
  )
}

/** Read-only view of the session in progress. Dim, no motion. */
function InProgress({ session }: { session: SleepSession }) {
  const now = useNow(30_000)
  const { reading } = useLive()
  const { targets } = useTargets()
  const elapsed = hoursMinutes((now - new Date(session.started_at).getTime()) / 60_000)
  const state = liveState(reading, targets)
  const stale = !!reading && minutesSince(reading.received_at, now) >= STALE_AFTER_MIN

  return (
    <main className="flex min-h-[calc(100vh-72px)] flex-col items-center justify-center gap-12 p-20 text-center">
      <div className="flex flex-col items-center gap-4">
        <div className="flex size-[120px] items-center justify-center rounded-full bg-surface-2">
          <Moon className="size-12 text-great" aria-hidden />
        </div>
        <p className="text-base tracking-[0.5px] text-text-muted">
          Sleep mode on since {timeOfDay(session.started_at)}
        </p>
      </div>

      <div className="flex flex-col items-center gap-2">
        <p className="text-[96px] leading-none font-thin tracking-[-3px] text-text">{elapsed}</p>
        <p className="text-sm tracking-[0.1px] text-text-dim uppercase">Time in sleep mode</p>
      </div>

      <div className="flex items-center gap-2 text-base text-text-muted">
        {state.kind === 'ok' && reading?.score ? (
          <>
            <span className={cn('size-1.5 rounded-full', BAND_BG[reading.score.band])} />
            <span>
              Score {tenths(reading.score.total)} · {BAND_LABEL[reading.score.band]}
            </span>
          </>
        ) : state.kind === 'warming-up' ? (
          <span>Score — · sensor warming up, scores in {state.minutesLeft} min</span>
        ) : (
          <span>Score —</span>
        )}
      </div>

      <div className={cn('flex gap-4', stale && 'opacity-40')}>
        {METRICS.map((m) => {
          const v = reading ? m.value(reading) : null
          return (
            <div key={m.key} className="flex items-center gap-2.5 rounded-full bg-surface px-5 py-3">
              <span className="text-base font-medium tracking-[-0.2px] text-text">
                {v === null ? '—' : withUnit(m, v)}
              </span>
              <span className="text-xs tracking-[0.15px] text-text-muted">{m.short}</span>
            </div>
          )
        })}
      </div>
      {stale && reading && (
        <p className="text-xs text-fair">Last reading {minutesSince(reading.received_at, now)} min ago</p>
      )}

      <p className="text-xs text-text-dim">To end sleep mode, hold the button on the device screen.</p>
    </main>
  )
}

function MorningSummary({ night }: { night: NightReport | null }) {
  const { navigate } = useRouter()
  const { targets } = useTargets()
  const greeting = new Date().getHours() < 12 ? 'Good morning' : 'Sleep mode ended'

  return (
    <main className="flex min-h-[calc(100vh-72px)] flex-col items-center justify-center gap-11 p-20">
      <div className="flex flex-col items-center gap-4">
        <div className="flex size-[120px] items-center justify-center rounded-full bg-surface-2">
          <Sun className="size-8 text-great" aria-hidden />
        </div>
        <div className="flex flex-col items-center gap-1">
          <h1 className="text-[22px] font-medium tracking-[-0.3px] text-text">{greeting}</h1>
          {night && <p className="text-[13px] tracking-[0.1px] text-text-muted uppercase">{longDate(night.ended_at)}</p>}
        </div>
      </div>

      {night ? (
        <>
          <div className="w-[500px] rounded-2xl border border-line bg-surface p-5">
            <NightScoreBlock night={night} />
          </div>
          <SummaryCard night={night} targets={targets} />
          <div className="flex gap-4">
            {METRICS.map((m) => {
              const s = night[statsKey(m.key)]
              return (
                <div key={m.key} className="flex items-center gap-2.5 rounded-full bg-surface px-5 py-3">
                  <span className="text-base font-medium tracking-[-0.2px] text-text">
                    {s ? withUnit(m, s.avg) : '—'}
                  </span>
                  <span className="text-xs tracking-[0.15px] text-text-muted">{m.short} avg</span>
                </div>
              )
            })}
          </div>
        </>
      ) : (
        <p className="text-sm text-text-muted">The session ended, but its report isn't available yet.</p>
      )}

      <div className="flex gap-4">
        <button
          type="button"
          onClick={() => navigate('/last-night')}
          className="rounded-full bg-great px-8 py-3 text-sm font-semibold text-bg hover:opacity-90"
        >
          View full report
        </button>
        <button
          type="button"
          onClick={() => navigate('/')}
          className="rounded-full border border-line px-8 py-3 text-sm font-semibold text-text-muted hover:text-text"
        >
          Dismiss
        </button>
      </div>
    </main>
  )
}

function SummaryCard({ night, targets }: { night: NightReport; targets: Targets | null }) {
  const { title, detail } = nightHeadline(night, targets)
  return (
    <div className="flex w-[600px] flex-col gap-3 rounded-2xl border border-line bg-surface p-6 text-center">
      <p className="text-sm font-semibold text-text">{title}</p>
      {detail && <p className="text-[13px] leading-[1.65] tracking-[0.1px] text-text-muted">{detail}</p>}
    </div>
  )
}
