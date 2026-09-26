// Client for POST /api/chat. The reply is a Server-Sent Events stream, but
// EventSource can't POST, so the stream is read from fetch and parsed here.
import { z } from 'zod'

export const ToolCallEvent = z.object({
  name: z.string(),
  arguments: z.unknown(),
  result: z.unknown(),
})
export type ToolCallEvent = z.infer<typeof ToolCallEvent>

export const Grounding = z.object({
  verified: z.boolean(),
  checked: z.number(),
  unmatched: z.array(z.string()),
})
export type Grounding = z.infer<typeof Grounding>

export type ChatEvent =
  | { type: 'tool_call'; data: ToolCallEvent }
  | { type: 'token'; text: string }
  | { type: 'done'; grounding: Grounding }
  | { type: 'offline'; message: string }

const schemas = {
  tool_call: ToolCallEvent,
  token: z.object({ text: z.string() }),
  done: z.object({ grounding: Grounding }),
  offline: z.object({ message: z.string() }),
}

function toEvent(name: string, data: string): ChatEvent | null {
  let json: unknown
  try {
    json = JSON.parse(data)
  } catch {
    return null
  }
  switch (name) {
    case 'tool_call': {
      const p = schemas.tool_call.safeParse(json)
      return p.success ? { type: 'tool_call', data: p.data } : null
    }
    case 'token': {
      const p = schemas.token.safeParse(json)
      return p.success ? { type: 'token', text: p.data.text } : null
    }
    case 'done': {
      const p = schemas.done.safeParse(json)
      return p.success ? { type: 'done', grounding: p.data.grounding } : null
    }
    case 'offline': {
      const p = schemas.offline.safeParse(json)
      return p.success ? { type: 'offline', message: p.data.message } : null
    }
    default:
      return null
  }
}

/** Parses complete SSE frames out of `buffer`; returns the events and the leftover text. */
export function parseSse(buffer: string): { events: ChatEvent[]; rest: string } {
  const events: ChatEvent[] = []
  const frames = buffer.split('\n\n')
  const rest = frames.pop() ?? ''
  for (const frame of frames) {
    let name = 'message'
    const data: string[] = []
    for (const line of frame.split('\n')) {
      if (line.startsWith('event:')) name = line.slice(6).trim()
      else if (line.startsWith('data:')) data.push(line.slice(5).trimStart())
    }
    if (data.length === 0) continue // keep-alive comments
    const event = toEvent(name, data.join('\n'))
    if (event) events.push(event)
  }
  return { events, rest }
}

export interface Turn {
  role: 'user' | 'assistant'
  content: string
}

export const OFFLINE_MESSAGE = 'Assistant offline — dashboard still live'

/** Optional shared token (CHAT_TOKEN on the backend), from ?token= in the URL. */
function chatToken(): string | null {
  try {
    const fromUrl = new URLSearchParams(window.location.search).get('token')
    if (fromUrl) localStorage.setItem('chatToken', fromUrl)
    return fromUrl ?? localStorage.getItem('chatToken')
  } catch {
    return null
  }
}

/** Sends a message and calls `onEvent` for each streamed event. */
export async function sendChat(
  message: string,
  history: Turn[],
  onEvent: (e: ChatEvent) => void,
  signal?: AbortSignal,
): Promise<void> {
  const headers: Record<string, string> = { 'content-type': 'application/json' }
  const token = chatToken()
  if (token) headers.authorization = `Bearer ${token}`

  let res: Response
  try {
    res = await fetch('/api/chat', {
      method: 'POST',
      headers,
      body: JSON.stringify({ message, history: history.slice(-20) }),
      signal,
    })
  } catch {
    onEvent({ type: 'offline', message: OFFLINE_MESSAGE })
    return
  }
  if (!res.ok || !res.body) {
    const message = res.status === 401 ? 'Chat needs an access token' : OFFLINE_MESSAGE
    onEvent({ type: 'offline', message })
    return
  }

  const reader = res.body.pipeThrough(new TextDecoderStream()).getReader()
  let buffer = ''
  let finished = false
  for (;;) {
    const { value, done } = await reader.read()
    if (done) break
    buffer += value
    const { events, rest } = parseSse(buffer)
    buffer = rest
    for (const e of events) {
      if (e.type === 'done' || e.type === 'offline') finished = true
      onEvent(e)
    }
  }
  // The stream ended without done/offline (connection dropped).
  if (!finished) onEvent({ type: 'offline', message: OFFLINE_MESSAGE })
}
