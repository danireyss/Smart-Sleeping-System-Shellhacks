import { Moon, Sun } from 'lucide-react'
import { useEffect, useRef, useState, type KeyboardEvent } from 'react'

import { NightScoreBlock } from '@/components/NightScore'
import { useLive, useNow, useTargets } from '@/hooks/data'
import { nightHeadline, statsKey } from '@/lib/night'
import { getLatestNight, getSleepCurrent, type NightReport, type SleepSession, type Targets } from '@/lib/api'
import {
  BAND_BG,
  METRICS,
  STALE_AFTER_MIN,
  hoursMinutes,
  liveState,
  longDate,
  minutesSince,
  tenths,
  withUnit,
} from '@/lib/format'
import { enterSleepMode, leaveSleepMode, requestFullscreen, useWakeLock } from '@/lib/sleep'
import { cn } from '@/lib/utils'
import { useRouter } from '@/router'

type View =
  | { kind: 'loading' }
  | { kind: 'idle' }
  | { kind: 'active'; session: SleepSession }
  | { kind: 'summary'; night: NightReport | null }

export function Sleep() {
  const [view, setView] = useState<View>({ kind: 'loading' })

  // Resume an open session after a reload or from another device.
  useEffect(() => {
    getSleepCurrent().then(
      (open) => setView(open ? { kind: 'active', session: open } : { kind: 'idle' }),
      () => setView({ kind: 'idle' }),
    )
  }, [])

  async function start() {
    const session = await enterSleepMode()
    setView({ kind: 'active', session })
  }

  async function end() {
    await leaveSleepMode()
    setView({ kind: 'summary', night: await getLatestNight() })
  }

  switch (view.kind) {
    case 'loading':
      return null
    case 'idle':
      return <Idle onStart={start} />
    case 'active':
      return <ActiveSleep session={view.session} onEnd={end} />
    case 'summary':
      return <MorningSummary night={view.night} />
  }
}

function Idle({ onStart }: { onStart: () => Promise<void> }) {
  const [busy, setBusy] = useState(false)
  return (
    <main className="flex min-h-[calc(100vh-72px)] flex-col items-center justify-center gap-8 p-20 text-center">
      <div className="flex size-[120px] items-center justify-center rounded-full bg-surface-2">
        <Moon className="size-12 text-great" aria-hidden />
      </div>
      <div className="flex max-w-md flex-col gap-2">
        <h1 className="text-[22px] font-medium tracking-[-0.3px] text-text">Sleep mode</h1>
        <p className="text-[13px] leading-[1.65] text-text-muted">
          Tracks the room from when you start until you end it. The screen goes dark and stays on.
        </p>
      </div>
      <button
        type="button"
        disabled={busy}
        onClick={() => {
          setBusy(true)
          onStart().finally(() => setBusy(false))
        }}
        className="flex items-center gap-3 rounded-full bg-great px-8 py-3 text-sm font-semibold text-bg hover:opacity-90 disabled:opacity-60"
      >
        <Moon className="size-4" aria-hidden />
        Start sleep mode
      </button>
    </main>
  )
}

/** Full screen, true black, dim text, no motion. Hold the corner button to end. */
function ActiveSleep({ session, onEnd }: { session: SleepSession; onEnd: () => Promise<void> }) {
  useWakeLock()
  const now = useNow(30_000)
  const { reading } = useLive()
  const { targets } = useTargets()
  const [fullscreen, setFullscreen] = useState(!!document.fullscreenElement)

  useEffect(() => {
    const onChange = () => setFullscreen(!!document.fullscreenElement)
    document.addEventListener('fullscreenchange', onChange)
    return () => document.removeEventListener('fullscreenchange', onChange)
  }, [])

  const elapsed = hoursMinutes((now - new Date(session.started_at).getTime()) / 60_000)
  const state = liveState(reading, targets)
  const stale = !!reading && minutesSince(reading.received_at, now) >= STALE_AFTER_MIN

  return (
    <div className="sleep-mode fixed inset-0 z-50 flex flex-col items-center justify-center gap-12 bg-black text-[#5a5f67] select-none">
      <div className="flex flex-col items-center gap-2">
        <p className="text-[96px] leading-none font-thin tracking-[-3px] text-[#6b7079]">{elapsed}</p>
        <p className="text-sm tracking-[0.1px]">Time in sleep mode</p>
      </div>

      <div className="flex items-center gap-2 text-base">
        {state.kind === 'ok' && reading?.score ? (
          <>
            <span className={cn('size-1.5 rounded-full opacity-60', BAND_BG[reading.score.band])} />
            <span>Score {tenths(reading.score.total)}</span>
          </>
        ) : state.kind === 'warming-up' ? (
          <span>Score — · sensor warming up, scores in {state.minutesLeft} min</span>
        ) : (
          <span>Score —</span>
        )}
      </div>

      <div className={cn('flex gap-10 text-base', stale && 'opacity-50')}>
        {METRICS.map((m) => {
          const v = reading ? m.value(reading) : null
          return (
            <p key={m.key}>
              {v === null ? '—' : withUnit(m, v)} <span className="text-xs">{m.short}</span>
            </p>
          )
        })}
      </div>
      {stale && reading && (
        <p className="text-xs">Last reading {minutesSince(reading.received_at, now)} min ago</p>
      )}

      {!fullscreen && (
        <button type="button" onClick={requestFullscreen} className="absolute top-6 left-6 text-xs text-[#3a3e44]">
          Full screen
        </button>
      )}
      <HoldToEnd onComplete={onEnd} />
    </div>
  )
}

const HOLD_MS = 1500

/** Press and hold for 1.5 s to end (mouse, touch, or Enter/Space). */
function HoldToEnd({ onComplete }: { onComplete: () => Promise<void> }) {
  const [progress, setProgress] = useState(0)
  const [ending, setEnding] = useState(false)
  const frame = useRef<number | null>(null)
  const startedAt = useRef<number | null>(null)

  const cancel = () => {
    if (frame.current !== null) cancelAnimationFrame(frame.current)
    frame.current = null
    startedAt.current = null
    setProgress(0)
  }

  const begin = () => {
    if (ending || startedAt.current !== null) return
    startedAt.current = performance.now()
    const tick = (t: number) => {
      const p = Math.min(1, (t - startedAt.current!) / HOLD_MS)
      setProgress(p)
      if (p >= 1) {
        frame.current = null
        startedAt.current = null
        setEnding(true)
        onComplete().catch(() => {
          setEnding(false)
          setProgress(0)
        })
      } else {
        frame.current = requestAnimationFrame(tick)
      }
    }
    frame.current = requestAnimationFrame(tick)
  }

  useEffect(() => cancel, [])

  const onKey = (e: KeyboardEvent, down: boolean) => {
    if (e.key !== 'Enter' && e.key !== ' ') return
    e.preventDefault()
    if (down && !e.repeat) begin()
    if (!down) cancel()
  }

  return (
    <button
      type="button"
      onPointerDown={begin}
      onPointerUp={cancel}
      onPointerLeave={cancel}
      onPointerCancel={cancel}
      onKeyDown={(e) => onKey(e, true)}
      onKeyUp={(e) => onKey(e, false)}
      onContextMenu={(e) => e.preventDefault()}
      aria-label="Hold to end sleep mode"
      className="absolute right-6 bottom-6 overflow-hidden rounded-full border border-[#1d2128] px-5 py-2.5 text-xs text-[#5a5f67]"
    >
      <span
        className="absolute inset-y-0 left-0 bg-[#1d2128]"
        style={{ width: `${progress * 100}%` }}
        aria-hidden
      />
      <span className="relative">{ending ? 'Ending…' : 'Hold to end sleep mode'}</span>
    </button>
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
