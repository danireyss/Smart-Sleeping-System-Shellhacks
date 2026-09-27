import { useEffect, useState } from 'react'

import {
  getCurrent,
  getLatestNight,
  getNight,
  getNights,
  getReadings,
  getSleepCurrent,
  getTargets,
  ScoredReading,
  SleepSession,
  type NightReport,
  type NightSummary,
  type ScoredReading as Reading,
  type Targets,
} from '@/lib/api'

/** Re-renders every `ms` so "N min ago" and warm-up countdowns stay current. */
export function useNow(ms = 30_000): number {
  const [now, setNow] = useState(() => Date.now())
  useEffect(() => {
    const id = setInterval(() => setNow(Date.now()), ms)
    return () => clearInterval(id)
  }, [ms])
  return now
}

/** Scoring targets from the backend; `failed` if they couldn't be loaded. */
export function useTargets(): { targets: Targets | null; failed: boolean } {
  const [targets, setTargets] = useState<Targets | null>(null)
  const [failed, setFailed] = useState(false)
  useEffect(() => {
    getTargets().then(setTargets, (e) => {
      console.error('targets', e)
      setFailed(true)
    })
  }, [])
  return { targets, failed }
}

export interface Live {
  /** undefined while loading, null when the backend has no readings yet. */
  reading: Reading | null | undefined
  /** True when the backend itself can't be reached. */
  unreachable: boolean
}

/** The latest reading: loaded once, then updated live from /api/stream (SSE). */
export function useLive(): Live {
  const [reading, setReading] = useState<Reading | null | undefined>(undefined)
  const [unreachable, setUnreachable] = useState(false)

  useEffect(() => {
    let cancelled = false
    const load = () =>
      getCurrent().then(
        (r) => {
          if (cancelled) return
          setReading(r)
          setUnreachable(false)
        },
        () => !cancelled && setUnreachable(true),
      )
    load()

    const stream = new EventSource('/api/stream')
    stream.addEventListener('reading', (e) => {
      const parsed = ScoredReading.safeParse(JSON.parse((e as MessageEvent).data))
      if (parsed.success) {
        setReading(parsed.data)
        setUnreachable(false)
      }
    })
    // EventSource reconnects on its own; reload on reconnect to catch up.
    stream.onopen = load
    stream.onerror = () => setUnreachable(stream.readyState === EventSource.CLOSED)

    // Safety net in case the stream silently stalls.
    const poll = setInterval(load, 60_000)
    return () => {
      cancelled = true
      stream.close()
      clearInterval(poll)
    }
  }, [])

  return { reading, unreachable }
}

export interface SleepState {
  /** The open session; null when sleep mode is off; undefined while loading. */
  session: SleepSession | null | undefined
  /** The session that ended while this page was open, if any. */
  justEnded: SleepSession | null
}

/**
 * Sleep mode as set on the device LCD: loaded once, then updated live from the
 * `sleep` events on /api/stream (sent on every start and end).
 */
export function useSleepSession(): SleepState {
  const [state, setState] = useState<SleepState>({ session: undefined, justEnded: null })

  useEffect(() => {
    let cancelled = false
    const load = () =>
      getSleepCurrent().then(
        (open) => !cancelled && setState((s) => ({ ...s, session: open })),
        (e) => console.error('sleep', e),
      )
    load()

    const stream = new EventSource('/api/stream')
    stream.addEventListener('sleep', (e) => {
      const parsed = SleepSession.safeParse(JSON.parse((e as MessageEvent).data))
      if (!parsed.success) return
      const s = parsed.data
      setState(s.ended_at === null ? { session: s, justEnded: null } : { session: null, justEnded: s })
    })
    stream.onopen = load // catch up after reconnecting
    const poll = setInterval(load, 60_000)
    return () => {
      cancelled = true
      stream.close()
      clearInterval(poll)
    }
  }, [])

  return state
}

/** Readings in [start, end), refetched when `refreshKey` changes. */
export function useReadings(start: Date | null, end: Date | null, refreshKey: unknown = 0) {
  const [readings, setReadings] = useState<Reading[] | null>(null)
  const startMs = start?.getTime()
  const endMs = end?.getTime()
  useEffect(() => {
    if (startMs === undefined || endMs === undefined) return
    let cancelled = false
    getReadings(new Date(startMs), new Date(endMs)).then(
      (r) => !cancelled && setReadings(r),
      (e) => console.error('readings', e),
    )
    return () => {
      cancelled = true
    }
  }, [startMs, endMs, refreshKey])
  return readings
}

/**
 * A night's full report: the one with `id`, or the latest when `id` is null.
 * undefined until the first load, null when there is no such night. While a
 * different night loads, the previous report stays on screen.
 */
export function useNight(id: number | null): NightReport | null | undefined {
  const [night, setNight] = useState<NightReport | null | undefined>(undefined)
  useEffect(() => {
    let cancelled = false
    ;(id === null ? getLatestNight() : getNight(id)).then(
      (n) => !cancelled && setNight(n),
      (e) => {
        console.error('night', e)
        if (!cancelled) setNight(null)
      },
    )
    return () => {
      cancelled = true
    }
  }, [id])
  return night
}

/**
 * Every finished night (newest first): undefined while loading, or null if the
 * list couldn't be loaded (e.g. an older backend without /api/nights).
 */
export function useNights(): NightSummary[] | null | undefined {
  const [nights, setNights] = useState<NightSummary[] | null | undefined>(undefined)
  useEffect(() => {
    getNights().then(setNights, (e) => {
      console.error('nights', e)
      setNights(null)
    })
  }, [])
  return nights
}
