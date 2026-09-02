import { useCallback, useEffect, useRef, useState } from "react"

import { daemonAuthHeaders, daemonEventsUrl } from "@/lib/daemon-auth"
import { isDaemonSnapshot } from "@/lib/api-validation"
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
  const body: unknown = await response.json()
  if (!isDaemonSnapshot(body)) {
    throw new Error("Snapshot response had an invalid shape")
  }
  return body
}

export function useDaemon(): DaemonState {
  const [snapshot, setSnapshot] = useState<DaemonSnapshot | null>(null)
  const [connection, setConnection] = useState<ConnectionState>("connecting")
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)
  const activeRequest = useRef<AbortController | null>(null)
  const requestGeneration = useRef(0)

  const refresh = useCallback(async () => {
    activeRequest.current?.abort()
    const controller = new AbortController()
    const generation = requestGeneration.current + 1
    activeRequest.current = controller
    requestGeneration.current = generation

    try {
      const next = await fetchSnapshot(controller.signal)
      if (generation !== requestGeneration.current) return
      setSnapshot(next)
      setError(null)
    } catch (reason) {
      if (reason instanceof DOMException && reason.name === "AbortError") return
      if (generation !== requestGeneration.current) return
      setError(
        reason instanceof Error
          ? reason.message
          : "Failed to load daemon snapshot"
      )
    } finally {
      if (generation === requestGeneration.current) {
        activeRequest.current = null
        setLoading(false)
      }
    }
  }, [])

  useEffect(() => {
    void refresh()

    const source = new EventSource(daemonEventsUrl("/api/events"))
    source.onopen = () => {
      setConnection("live")
      void refresh()
    }
    source.onerror = () => setConnection("offline")
    const handleScanEvent = () => void refresh()
    source.addEventListener("scan_started", handleScanEvent)
    source.addEventListener("scan_completed", handleScanEvent)
    source.addEventListener("scan_failed", handleScanEvent)

    return () => {
      requestGeneration.current += 1
      activeRequest.current?.abort()
      activeRequest.current = null
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
