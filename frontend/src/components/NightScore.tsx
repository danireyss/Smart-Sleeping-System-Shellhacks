import type { NightReport } from '@/lib/api'
import { BAND_LABEL, BAND_TEXT, hoursMinutes } from '@/lib/format'
import { cn } from '@/lib/utils'

const CIRCLE_BG = {
  great: 'bg-great/15',
  good: 'bg-good/15',
  fair: 'bg-fair/15',
  poor: 'bg-poor/15',
} as const

/** The nightly score circle, or the incomplete-night message in its place. */
export function NightScoreBlock({ night, align = 'left' }: { night: NightReport; align?: 'left' | 'right' }) {
  const scored = !night.incomplete && night.score !== null && night.band !== null
  const text = (
    <div className={cn('flex flex-col gap-1 whitespace-nowrap', align === 'right' && 'items-end text-right')}>
      <p className="text-[11px] font-medium tracking-[1.5px] text-text-muted uppercase">
        Overnight environment score
      </p>
      {scored ? (
        <p className={cn('text-xl font-medium tracking-[-0.2px]', BAND_TEXT[night.band!])}>
          {BAND_LABEL[night.band!]} sleep environment
        </p>
      ) : (
        <p className="text-xl font-medium text-fair">
          Incomplete night ({night.completeness_pct.toFixed(1)}% of readings)
        </p>
      )}
      <p className="text-sm text-text-muted">Time in sleep mode: {hoursMinutes(night.duration_minutes)}</p>
      {night.short_session && (
        <p className="text-sm text-fair">Under 1 hour: too short to be meaningful.</p>
      )}
    </div>
  )
  const circle = scored && (
    <div
      className={cn('flex size-20 shrink-0 items-center justify-center rounded-full', CIRCLE_BG[night.band!])}
    >
      <p className={cn('text-[34px] leading-none font-thin tracking-[-1px]', BAND_TEXT[night.band!])}>
        {night.score!.toFixed(1)}
      </p>
    </div>
  )
  return (
    <div className="flex shrink-0 items-center gap-6">
      {align === 'left' ? (
        <>
          {circle}
          {text}
        </>
      ) : (
        <>
          {text}
          {circle}
        </>
      )}
    </div>
  )
}
