import { CloudMoon } from 'lucide-react'

import { cn } from '@/lib/utils'
import { type Route, useRouter } from '@/router'

const TABS: { route: Route; label: string }[] = [
  { route: '/', label: 'Home' },
  { route: '/sleep', label: 'Sleep' },
  { route: '/last-night', label: 'Last night' },
]

export function Nav() {
  const { route, navigate } = useRouter()
  return (
    <header className="flex h-[72px] items-center justify-between border-b border-line px-10">
      <div className="flex items-center gap-2.5">
        <CloudMoon className="size-5 text-great" aria-hidden />
        <p className="text-base font-medium tracking-[2px] text-text">
          HYPNOS
        </p>
      </div>
      <nav className="flex items-center gap-2 rounded-full bg-surface-2 p-1" aria-label="Main">
        {TABS.map((tab) => (
          <button
            key={tab.route}
            type="button"
            onClick={() => navigate(tab.route)}
            aria-current={route === tab.route ? 'page' : undefined}
            className={cn(
              'rounded-full px-5 py-2 text-[13px] font-medium tracking-[0.2px] transition-colors',
              route === tab.route ? 'bg-surface text-text' : 'text-text-muted hover:text-text',
            )}
          >
            {tab.label}
          </button>
        ))}
      </nav>
      <p className="text-xs tracking-[0.15px] text-text-muted">Master Bedroom</p>
    </header>
  )
}
