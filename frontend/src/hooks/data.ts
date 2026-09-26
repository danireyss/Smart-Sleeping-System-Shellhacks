import { useEffect, useState } from 'react'

import {
  getCurrent,
  getLatestNight,
  getReadings,
  getTargets,
  ScoredReading,
  type NightReport,
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

/** undefined while loading, null when no session has ended yet. */
export function useLatestNight(): NightReport | null | undefined {
  const [night, setNight] = useState<NightReport | null | undefined>(undefined)
  useEffect(() => {
    getLatestNight().then(setNight, (e) => {
      console.error('night', e)
      setNight(null)
    })
  }, [])
  return night
}
