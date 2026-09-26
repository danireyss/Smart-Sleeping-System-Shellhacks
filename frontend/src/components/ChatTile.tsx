import { ArrowRight, Sparkles } from 'lucide-react'
import { useEffect, useRef, useState, type FormEvent } from 'react'

import { Collapsible, CollapsibleContent, CollapsibleTrigger } from '@/components/ui/collapsible'
import { OFFLINE_MESSAGE, sendChat, type Grounding, type ToolCallEvent, type Turn } from '@/lib/chat'
import { cn } from '@/lib/utils'

const TOOL_LABELS: Record<string, string> = {
  get_current: 'current readings',
  get_summary: 'time-range summary',
  get_night_latest: "last night's report",
  get_targets: 'targets',
}

const SUGGESTIONS = ['Why is the score what it is?', 'How was last night?', 'Where do the targets come from?']

interface Message {
  role: 'user' | 'assistant'
  text: string
  tools: ToolCallEvent[]
  grounding?: Grounding
  pending?: boolean
}

export function ChatTile({ className }: { className?: string }) {
  const [messages, setMessages] = useState<Message[]>([])
  const [input, setInput] = useState('')
  const [busy, setBusy] = useState(false)
  const [offline, setOffline] = useState<string | null>(null)
  const scrollRef = useRef<HTMLDivElement>(null)

  useEffect(() => {
    scrollRef.current?.scrollTo({ top: scrollRef.current.scrollHeight })
  }, [messages])

  const updateLast = (fn: (m: Message) => Message) =>
    setMessages((ms) => [...ms.slice(0, -1), fn(ms[ms.length - 1])])

  async function send(text: string) {
    const message = text.trim()
    if (!message || busy || offline) return
    const history: Turn[] = messages
      .filter((m) => !m.pending && m.text)
      .map((m) => ({ role: m.role, content: m.text }))
    setInput('')
    setBusy(true)
    setMessages((ms) => [
      ...ms,
      { role: 'user', text: message, tools: [] },
      { role: 'assistant', text: '', tools: [], pending: true },
    ])
    await sendChat(message, history, (e) => {
      switch (e.type) {
        case 'tool_call':
          updateLast((m) => ({ ...m, tools: [...m.tools, e.data] }))
          break
        case 'token':
          updateLast((m) => ({ ...m, text: m.text + e.text }))
          break
        case 'done':
          updateLast((m) => ({ ...m, grounding: e.grounding, pending: false }))
          break
        case 'offline':
          setOffline(e.message)
          // Drop the unanswered placeholder; keep any partial text.
          setMessages((ms) => {
            const last = ms[ms.length - 1]
            return last.text ? [...ms.slice(0, -1), { ...last, pending: false }] : ms.slice(0, -1)
          })
          break
      }
    })
    setBusy(false)
  }

  function onSubmit(e: FormEvent) {
    e.preventDefault()
    void send(input)
  }

  return (
    <section
      className={cn('flex h-[620px] flex-col rounded-2xl border border-line bg-surface', className)}
      aria-label="Ask about your room"
    >
      <header className="flex items-center justify-between border-b border-line p-5">
        <h2 className="text-sm font-medium tracking-[0.3px] text-text">Ask about your room</h2>
        <Sparkles className="size-4 text-great" aria-hidden />
      </header>

      <div ref={scrollRef} className="flex flex-1 flex-col gap-4 overflow-y-auto p-5" aria-live="polite">
        {messages.length === 0 && (
          <div className="flex flex-col gap-2">
            <p className="text-[13px] text-text-muted">
              Answers use live readings. Every number is checked against the data the assistant looked up.
            </p>
            {SUGGESTIONS.map((s) => (
              <button
                key={s}
                type="button"
                disabled={busy || !!offline}
                onClick={() => void send(s)}
                className="w-fit rounded-md bg-surface-3 px-2.5 py-1.5 text-left text-xs text-text-muted hover:text-text disabled:opacity-50"
              >
                {s}
              </button>
            ))}
          </div>
        )}
        {messages.map((m, i) =>
          m.role === 'user' ? (
            <div key={i} className="flex justify-end">
              <p className="max-w-[320px] rounded-xl rounded-br-sm bg-line p-3 text-[13px] leading-[1.65] text-text">
                {m.text}
              </p>
            </div>
          ) : (
            <AssistantMessage key={i} message={m} />
          ),
        )}
        {offline && (
          <div className="flex items-center justify-between gap-3 rounded-lg border border-line p-3 text-xs text-text-muted">
            <span>{offline}</span>
            <button type="button" className="text-great hover:underline" onClick={() => setOffline(null)}>
              Try again
            </button>
          </div>
        )}
      </div>

      <form onSubmit={onSubmit} className="border-t border-line p-4">
        <label className="flex items-center gap-2 rounded-lg bg-bg px-4 py-2.5 focus-within:ring-1 focus-within:ring-great">
          <span className="sr-only">Message</span>
          <input
            value={input}
            onChange={(e) => setInput(e.target.value)}
            disabled={busy || !!offline}
            placeholder={offline ? OFFLINE_MESSAGE : 'Message Aura…'}
            maxLength={2000}
            className="flex-1 bg-transparent text-[13px] text-text outline-none placeholder:text-text-dim disabled:cursor-not-allowed"
          />
          <button type="submit" disabled={busy || !!offline || !input.trim()} aria-label="Send">
            <ArrowRight className="size-4 text-text-muted" />
          </button>
        </label>
      </form>
    </section>
  )
}

function AssistantMessage({ message }: { message: Message }) {
  return (
    <div className="flex flex-col gap-2.5">
      <div className="rounded-xl rounded-bl-sm border border-line bg-bg p-4 text-[13px] leading-[1.65] whitespace-pre-wrap text-text">
        {message.text || <span className="text-text-muted">{message.tools.length ? 'Checking…' : 'Thinking…'}</span>}
      </div>
      {(message.tools.length > 0 || message.grounding) && (
        <div className="flex flex-wrap items-start gap-2">
          {message.tools.map((t, i) => (
            <ToolChip key={i} tool={t} />
          ))}
          {message.grounding && <GroundingBadge grounding={message.grounding} />}
        </div>
      )}
    </div>
  )
}

function ToolChip({ tool }: { tool: ToolCallEvent }) {
  return (
    <Collapsible className="max-w-full">
      <CollapsibleTrigger className="rounded-md bg-surface-3 px-2 py-1 text-[11px] tracking-[0.2px] text-text-muted hover:text-text">
        Checked: {TOOL_LABELS[tool.name] ?? tool.name}
      </CollapsibleTrigger>
      <CollapsibleContent className="data-[state=open]:animate-in data-[state=open]:fade-in-0">
        <dl className="mt-1.5 grid max-h-48 grid-cols-[auto_1fr] gap-x-3 gap-y-0.5 overflow-y-auto rounded-md bg-bg p-2.5 text-[11px]">
          {flatten(tool.result).map(([k, v]) => (
            <div key={k} className="contents">
              <dt className="text-text-muted">{k}</dt>
              <dd className="text-text">{v}</dd>
            </div>
          ))}
        </dl>
      </CollapsibleContent>
    </Collapsible>
  )
}

function GroundingBadge({ grounding }: { grounding: Grounding }) {
  if (grounding.verified) {
    return <span className="px-2 py-1 text-[11px] font-semibold tracking-[0.2px] text-great">✓ numbers verified</span>
  }
  return (
    <span
      className="px-2 py-1 text-[11px] font-semibold tracking-[0.2px] text-fair"
      title={`Not found in the data it checked: ${grounding.unmatched.join(', ')}`}
    >
      ⚠ unverified ({grounding.unmatched.join(', ')})
    </span>
  )
}

/** Flattens a tool result into "path: value" rows, exactly as returned. */
function flatten(value: unknown, prefix = ''): [string, string][] {
  if (value === null || typeof value !== 'object') {
    return [[prefix || 'value', value === null ? '—' : String(value)]]
  }
  if (Array.isArray(value)) {
    return [[prefix, value.map((v) => (typeof v === 'object' ? JSON.stringify(v) : String(v))).join(', ') || '—']]
  }
  return Object.entries(value).flatMap(([k, v]) => flatten(v, prefix ? `${prefix}.${k}` : k))
}
