import fs from "node:fs"
import os from "node:os"
import path from "node:path"

import tailwindcss from "@tailwindcss/vite"
import react from "@vitejs/plugin-react"
import { defineConfig } from "vitest/config"

const DAEMON_PORT = Number(process.env.REPOSCOUT_DAEMON_PORT ?? "7331")
const DAEMON_TOKEN_PATTERN = /^[0-9a-f]{64}$/
const MAX_DAEMON_TOKEN_FILE_BYTES = 128

function daemonTokenCandidates(port: number): string[] {
  const explicit = process.env.REPOSCOUT_DAEMON_TOKEN_FILE
  const home = os.homedir()
  const xdgRuntime = process.env.XDG_RUNTIME_DIR
  const xdgCache = process.env.XDG_CACHE_HOME
  // Windows: directories::ProjectDirs::cache_dir() → %LOCALAPPDATA%\reposcout\cache
  const localAppData = process.env.LOCALAPPDATA
  return [
    explicit,
    xdgRuntime ? path.join(xdgRuntime, "reposcout", `daemon-${port}.token`) : null,
    // macOS directories::ProjectDirs cache fallback (runtime_dir is None).
    path.join(home, "Library", "Caches", "reposcout", `daemon-${port}.token`),
    xdgCache
      ? path.join(xdgCache, "reposcout", `daemon-${port}.token`)
      : path.join(home, ".cache", "reposcout", `daemon-${port}.token`),
    localAppData
      ? path.join(localAppData, "reposcout", "cache", `daemon-${port}.token`)
      : null,
  ].filter((value): value is string => Boolean(value))
}

function readDaemonToken(port: number): string | undefined {
  for (const candidate of daemonTokenCandidates(port)) {
    let file: number | undefined
    try {
      const metadata = fs.lstatSync(candidate)
      if (!metadata.isFile() || metadata.size > MAX_DAEMON_TOKEN_FILE_BYTES) continue

      const unixFlags = fs.constants.O_NOFOLLOW | fs.constants.O_NONBLOCK
      const flags = fs.constants.O_RDONLY | (process.platform === "win32" ? 0 : unixFlags)
      file = fs.openSync(candidate, flags)
      const openedMetadata = fs.fstatSync(file)
      if (!openedMetadata.isFile() || openedMetadata.size > MAX_DAEMON_TOKEN_FILE_BYTES) continue

      const contents = Buffer.alloc(MAX_DAEMON_TOKEN_FILE_BYTES)
      const bytesRead = fs.readSync(file, contents, 0, contents.length, 0)
      const raw = contents.subarray(0, bytesRead).toString("utf8").trim()
      if (DAEMON_TOKEN_PATTERN.test(raw)) return raw
    } catch {
      // try next candidate
    } finally {
      if (file !== undefined) fs.closeSync(file)
    }
  }
  return undefined
}

export default defineConfig({
  appType: "spa",
  plugins: [react(), tailwindcss()],
  resolve: {
    alias: {
      "@": path.resolve(__dirname, "./src"),
    },
  },
  server: {
    proxy: {
      "/api": {
        target: `http://127.0.0.1:${DAEMON_PORT}`,
        configure: (proxy) => {
          proxy.on("proxyReq", (proxyReq) => {
            // Inject the token server-side so it never needs a VITE_* browser env.
            if (proxyReq.getHeader("authorization")) return
            const token = readDaemonToken(DAEMON_PORT)
            if (token) {
              proxyReq.setHeader("Authorization", `Bearer ${token}`)
            }
          })
        },
      },
    },
  },
  test: {
    environment: "jsdom",
    setupFiles: "./src/test/setup.ts",
    css: true,
  },
})
