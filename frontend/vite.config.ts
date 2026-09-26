import tailwindcss from '@tailwindcss/vite'
import react from '@vitejs/plugin-react'
import { defineConfig } from 'vite'

// `npm run dev` proxies /api to the board (or BACKEND_URL). `npm run build`
// writes dist/, which the Rust backend embeds and serves at /.
export default defineConfig({
  plugins: [react(), tailwindcss()],
  resolve: {
    alias: { '@': `${import.meta.dirname}/src` },
  },
  // The bundle (mostly Recharts) is served from the board on the local network.
  build: { chunkSizeWarningLimit: 1000 },
  server: {
    proxy: {
      '/api': { target: process.env.BACKEND_URL ?? 'http://danmig.local:8080', changeOrigin: true },
    },
  },
})
