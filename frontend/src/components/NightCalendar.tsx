import { ChevronLeft, ChevronRight } from 'lucide-react'
import { useMemo, useState } from 'react'

import type { Band, NightSummary } from '@/lib/api'
import { BAND_LABEL, hoursMinutes, tenths } from '@/lib/format'
import { dateKey, nightDate, nightsByDate } from '@/lib/night'
import { cn } from '@/lib/utils'

const CELL: Record<Band, string> = {
  great: 'bg-great/20 text-great',
  good: 'bg-good/20 text-good',
  fair: 'bg-fair/20 text-fair',
  poor: 'bg-poor/20 text-poor',
}
const DOT: Record<Band, string> = {
  great: 'bg-great',
  good: 'bg-good',
  fair: 'bg-fair',
  poor: 'bg-poor',
}
const WEEKDAYS = ['S', 'M', 'T', 'W', 'T', 'F', 'S']

/** Month calendar of past nights, each colored by its sleep-environment band. */
export function NightCalendar({
  nights,
  selectedId,
  onSelect,
}: {
  nights: NightSummary[]
  selectedId: number | null
  onSelect: (id: number) => void
}) {
  const byDate = useMemo(() => nightsByDate(nights), [nights])

  // Start on the month of the selected night (or the latest one).
  const initial = useMemo(() => {
    const n = nights.find((x) => x.session_id === selectedId) ?? nights[0]
    const [y, m] = (n ? nightDate(n.started_at) : dateKey(new Date())).split('-').map(Number)
    return { year: y, month: m - 1 }
  }, [nights, selectedId])
  const [view, setView] = useState(initial)

  const first = new Date(view.year, view.month, 1)
  const daysInMonth = new Date(view.year, view.month + 1, 0).getDate()
  const cells: (number | null)[] = [
    ...Array<null>(first.getDay()).fill(null),
    ...Array.from({ length: daysInMonth }, (_, i) => i + 1),
  ]
  const today = dateKey(new Date())
  const shift = (delta: number) => {
    const d = new Date(view.year, view.month + delta, 1)
    setView({ year: d.getFullYear(), month: d.getMonth() })
  }

  return (
    <section className="flex flex-col gap-4 rounded-2xl border border-line bg-surface p-5" aria-label="Night history">
      <div className="flex items-center justify-between">
        <h2 className="text-sm font-medium tracking-[0.3px] text-text">
          {first.toLocaleDateString([], { month: 'long', year: 'numeric' })}
        </h2>
        <div className="flex gap-1">
          <button type="button" onClick={() => shift(-1)} aria-label="Previous month" className="rounded-md p-1 text-text-muted hover:bg-surface-2 hover:text-text">
            <ChevronLeft className="size-4" />
          </button>
          <button type="button" onClick={() => shift(1)} aria-label="Next month" className="rounded-md p-1 text-text-muted hover:bg-surface-2 hover:text-text">
            <ChevronRight className="size-4" />
          </button>
        </div>
      </div>

      <div className="grid grid-cols-7 gap-1 text-center">
        {WEEKDAYS.map((d, i) => (
          <span key={i} className="pb-1 text-[11px] text-text-dim">
            {d}
          </span>
        ))}
        {cells.map((day, i) => {
          if (day === null) return <span key={`blank-${i}`} />
          const key = dateKey(new Date(view.year, view.month, day))
          const night = byDate.get(key)
          if (!night) {
            return (
              <span
                key={key}
                className={cn(
                  'flex aspect-square items-center justify-center rounded-lg text-xs text-text-dim',
                  key === today && 'ring-1 ring-line',
                )}
              >
                {day}
              </span>
            )
          }
          const scored = !night.incomplete && night.band !== null && night.score !== null
          const label = scored
            ? `Night of ${key}: ${tenths(night.score!)} ${BAND_LABEL[night.band!]}, ${hoursMinutes(night.duration_minutes)}`
            : `Night of ${key}: incomplete (${night.completeness_pct.toFixed(1)}% of readings)`
          return (
            <button
              key={key}
              type="button"
              onClick={() => onSelect(night.session_id)}
              title={label}
              aria-label={label}
              aria-pressed={night.session_id === selectedId}
              className={cn(
                'flex aspect-square flex-col items-center justify-center rounded-lg text-xs font-medium transition-colors',
                scored ? CELL[night.band!] : 'border border-dashed border-text-dim text-text-muted',
                night.short_session && 'opacity-60',
                night.session_id === selectedId && 'ring-2 ring-text',
              )}
            >
              <span>{day}</span>
              {scored && <span className="text-[9px] leading-none opacity-80">{tenths(night.score!)}</span>}
            </button>
          )
        })}
      </div>

      <div className="flex flex-wrap gap-x-3 gap-y-1 text-[11px] text-text-muted">
        {(['great', 'good', 'fair', 'poor'] as Band[]).map((b) => (
          <span key={b} className="flex items-center gap-1.5">
            <span className={cn('size-2 rounded-full', DOT[b])} />
            {BAND_LABEL[b]}
          </span>
        ))}
        <span className="flex items-center gap-1.5">
          <span className="size-2 rounded-full border border-dashed border-text-dim" />
          Incomplete
        </span>
      </div>
    </section>
  )
}
