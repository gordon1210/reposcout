import fs from "node:fs"
import os from "node:os"
import path from "node:path"

import { afterEach, describe, expect, it, vi } from "vitest"

interface ProxyRequest {
  getHeader(name: string): string | undefined
  setHeader(name: string, value: string): void
}

type ProxyRequestHandler = (request: ProxyRequest) => void

interface ProxyConfiguration {
  configure(proxy: {
    on(event: "proxyReq", handler: ProxyRequestHandler): void
  }): void
}

const temporaryDirectories: string[] = []

afterEach(() => {
  vi.unstubAllEnvs()
  vi.resetModules()
  for (const directory of temporaryDirectories.splice(0)) {
    fs.rmSync(directory, { recursive: true, force: true })
  }
})

describe("Vite daemon proxy authentication", () => {
  const unixIt = process.platform === "win32" ? it.skip : it

  unixIt("does not forward contents read through a symlinked token file", async () => {
    const directory = fs.mkdtempSync(path.join(os.tmpdir(), "reposcout-vite-token-"))
    temporaryDirectories.push(directory)
    const target = path.join(directory, "target")
    const tokenFile = path.join(directory, "daemon.token")
    fs.writeFileSync(target, `${"a".repeat(64)}\n`)
    fs.symlinkSync(target, tokenFile)

    vi.stubEnv("REPOSCOUT_DAEMON_PORT", "65534")
    vi.stubEnv("REPOSCOUT_DAEMON_TOKEN_FILE", tokenFile)
    vi.stubEnv("XDG_RUNTIME_DIR", directory)
    vi.stubEnv("XDG_CACHE_HOME", directory)
    vi.stubEnv("LOCALAPPDATA", directory)
    vi.resetModules()

    const config = (await import("./vite.config")).default as {
      server: {
        proxy: Record<string, ProxyConfiguration>
      }
    }
    let handler: ProxyRequestHandler | undefined
    config.server.proxy["/api"].configure({
      on: (_event, nextHandler) => {
        handler = nextHandler
      },
    })
    expect(handler).toBeDefined()

    const setHeader = vi.fn()
    handler?.({
      getHeader: () => undefined,
      setHeader,
    })

    expect(setHeader).not.toHaveBeenCalled()
  })
})
