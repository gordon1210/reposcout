import { useCallback, useEffect, useState } from "react"

import { daemonAuthHeaders, daemonEventsUrl } from "@/lib/daemon-auth"
import type { DaemonSnapshot } from "@/lib/types"

export type ConnectionState = "connecting" | "live" | "offline"

interface DaemonState {
  snapshot: DaemonSnapshot | null
  connection: ConnectionState
  loading: boolean
  error: string | null
  rescan: () => Promise<void>
}

async function fetchSnapshot(signal?: AbortSignal): Promise<DaemonSnapshot> {
  const response = await fetch("/api/snapshot", {
    signal,
    headers: daemonAuthHeaders(),
  })
  if (!response.ok) {
    throw new Error(`Snapshot request failed (${response.status})`)
  }
  return (await response.json()) as DaemonSnapshot
}

export function useDaemon(): DaemonState {
  const [snapshot, setSnapshot] = useState<DaemonSnapshot | null>(null)
  const [connection, setConnection] = useState<ConnectionState>("connecting")
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)

  const refresh = useCallback(async (signal?: AbortSignal) => {
    try {
      const next = await fetchSnapshot(signal)
      setSnapshot(next)
      setError(null)
    } catch (reason) {
      if (reason instanceof DOMException && reason.name === "AbortError") return
      setError(reason instanceof Error ? reason.message : "Failed to load daemon snapshot")
    } finally {
      if (!signal?.aborted) setLoading(false)
    }
  }, [])

  useEffect(() => {
    const controller = new AbortController()
    void refresh(controller.signal)

    const source = new EventSource(daemonEventsUrl("/api/events"))
    source.onopen = () => setConnection("live")
    source.onerror = () => setConnection("offline")
    const handleScanEvent = () => void refresh()
    source.addEventListener("scan_started", handleScanEvent)
    source.addEventListener("scan_completed", handleScanEvent)
    source.addEventListener("scan_failed", handleScanEvent)

    return () => {
      controller.abort()
      source.close()
    }
  }, [refresh])

  const rescan = useCallback(async () => {
    const response = await fetch("/api/rescan", {
      method: "POST",
      headers: daemonAuthHeaders({ "X-RepoScout-Request": "rescan" }),
    })
    if (!response.ok) {
      const message = `Rescan request failed (${response.status})`
      setError(message)
      throw new Error(message)
    }
    await refresh()
  }, [refresh])

  return { snapshot, connection, loading, error, rescan }
}
