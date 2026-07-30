import { useCallback, useEffect, useState } from "react"

import { daemonAuthHeaders } from "@/lib/daemon-auth"
import { isDaemonGraphResponse } from "@/lib/api-validation"
import type { DaemonGraphResponse, DependencyGraph } from "@/lib/types"

interface RepositoryGraphState {
  graph: DependencyGraph | null
  loading: boolean
  error: string | null
  retry: () => void
}

async function fetchGraph(
  revision: number,
  signal: AbortSignal
): Promise<DaemonGraphResponse> {
  const response = await fetch(`/api/graph?revision=${revision}`, {
    signal,
    headers: daemonAuthHeaders(),
  })
  if (!response.ok) {
    const body: unknown = await response.json().catch(() => null)
    const message =
      typeof body === "object" &&
      body !== null &&
      "error" in body &&
      typeof body.error === "string"
        ? body.error
        : `Graph request failed (${response.status})`
    throw new Error(message)
  }
  const body: unknown = await response.json()
  if (!isDaemonGraphResponse(body)) {
    throw new Error("Graph response had an invalid shape")
  }
  return body
}

export function useRepositoryGraph(revision: number): RepositoryGraphState {
  const [graph, setGraph] = useState<DependencyGraph | null>(null)
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)
  const [request, setRequest] = useState(0)

  useEffect(() => {
    const controller = new AbortController()
    setLoading(true)
    setError(null)
    setGraph(null)

    void fetchGraph(revision, controller.signal)
      .then((response) => {
        if (response.revision !== revision) {
          throw new Error(
            "The repository changed while its graph was being built. Try again."
          )
        }
        setGraph(response.graph)
      })
      .catch((reason: unknown) => {
        if (reason instanceof DOMException && reason.name === "AbortError")
          return
        setError(
          reason instanceof Error
            ? reason.message
            : "Failed to build repository graph"
        )
      })
      .finally(() => {
        if (!controller.signal.aborted) setLoading(false)
      })

    return () => controller.abort()
  }, [request, revision])

  const retry = useCallback(() => setRequest((current) => current + 1), [])
  return { graph, loading, error, retry }
}
