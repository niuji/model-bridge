import { defineConfig } from 'vite'
import vue from '@vitejs/plugin-vue'
import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'

// Single source of truth: the package version in Cargo.toml (which tracks the
// release tag). Injected at build time so the footer never drifts from the
// binary version. See CLAUDE.md "Releases" — Cargo.toml is bumped before the
// frontend build, so the embedded bundle always carries the tag version.
const appVersion = readFileSync(fileURLToPath(new URL('../Cargo.toml', import.meta.url)), 'utf8')
  .match(/^version = "(.+)"$/m)?.[1] ?? '0.0.0'

export default defineConfig({
  plugins: [vue()],
  define: { __APP_VERSION__: JSON.stringify(appVersion) },
  server: {
    port: 3000,
    proxy: {
      '/api': 'http://localhost:10020',
    },
  },
  build: {
    outDir: 'dist',
    assetsDir: 'assets',
  },
})
