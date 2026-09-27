# Hypnos web UI

Vite + React + TypeScript + Tailwind v4 + shadcn/ui, with zod validating every API
response and Recharts for charts. Design: `../docs/DESIGN.md` and the Figma frames.

```bash
npm install
npm run dev     # http://localhost:5173, proxies /api to the board (BACKEND_URL to override)
npm run build   # writes dist/, which the Rust backend embeds; commit dist/ after building
```

The board never runs Node: `cargo build` embeds `dist/` into the backend binary, which
serves it at `/` (and `/sleep`, `/last-night`). If `CHAT_TOKEN` is set on the backend,
open the UI once with `?token=<token>`; it is kept in localStorage.
