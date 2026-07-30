import { renderHook, waitFor } from "@testing-library/react"
import { afterEach, describe, expect, it, vi } from "vitest"

import { useRepositoryGraph } from "@/hooks/use-repository-graph"
import type { DependencyGraph } from "@/lib/types"

const graph: DependencyGraph = {
  languages: ["TypeScript"],
  nodes: 1,
  edges: 0,
  files: [
    { path: "src/app.ts", language: "TypeScript", fan_in: 0, fan_out: 0 },
  ],
  edge_list: [],
  cycles: [],
  orphans: [],
  top_depended: [],
  most_dependent: [],
  unresolved_imports: 0,
}

afterEach(() => vi.unstubAllGlobals())

describe("useRepositoryGraph", () => {
  it("requests and accepts only the mounted report revision", async () => {
    const fetch = vi.fn().mockResolvedValue({
      ok: true,
      json: vi.fn().mockResolvedValue({ revision: 9, graph }),
    })
    vi.stubGlobal("fetch", fetch)

    const { result } = renderHook(() => useRepositoryGraph(9))

    await waitFor(() => expect(result.current.loading).toBe(false))
    expect(fetch).toHaveBeenCalledWith("/api/graph?revision=9", {
      signal: expect.any(AbortSignal),
      headers: expect.any(Headers),
    })
    expect(result.current.graph?.files[0].path).toBe("src/app.ts")
    expect(result.current.error).toBeNull()
  })
})
