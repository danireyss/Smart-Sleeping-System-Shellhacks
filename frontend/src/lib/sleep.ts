// Entering and leaving sleep mode: the session on the backend, true full screen
// (Fullscreen API), and keeping the screen on (Screen Wake Lock).
import { useEffect } from 'react'

import { endSleep, getSleepCurrent, startSleep, type SleepSession } from './api'

/**
 * Starts a session (or resumes the open one) and goes full screen. Call it from
 * a click handler: browsers only allow full screen during a user gesture, so
 * the request is made before anything is awaited.
 */
export async function enterSleepMode(): Promise<SleepSession> {
  requestFullscreen()
  try {
    return await startSleep()
  } catch {
    // 409: a session is already open, so resume it.
    const open = await getSleepCurrent()
    if (open) return open
    throw new Error('could not start sleep mode')
  }
}

export async function leaveSleepMode(): Promise<SleepSession> {
  const ended = await endSleep()
  if (document.fullscreenElement) await document.exitFullscreen().catch(() => {})
  return ended
}

export function requestFullscreen() {
  if (!document.fullscreenElement) {
    document.documentElement.requestFullscreen?.().catch(() => {
      // Not allowed (no gesture, or unsupported): sleep mode still works.
    })
  }
}

/** Keeps the screen awake while mounted, re-acquiring after the tab is hidden. */
export function useWakeLock() {
  useEffect(() => {
    if (!('wakeLock' in navigator)) return
    let lock: WakeLockSentinel | null = null
    let released = false
    const acquire = async () => {
      try {
        lock = await navigator.wakeLock.request('screen')
      } catch {
        // Denied (e.g. low battery); the page still works.
      }
    }
    const onVisible = () => {
      if (document.visibilityState === 'visible' && !released) void acquire()
    }
    void acquire()
    document.addEventListener('visibilitychange', onVisible)
    return () => {
      released = true
      document.removeEventListener('visibilitychange', onVisible)
      void lock?.release()
    }
  }, [])
}
