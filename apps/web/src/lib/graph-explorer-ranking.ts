import type { GraphEdge } from "@/lib/types"

export function fileMatchRank(path: string, query: string): number {
  const normalized = path.toLowerCase()
  const name = normalized.split("/").at(-1) ?? normalized
  if (normalized === query) return 0
  if (name === query) return 1
  if (name.startsWith(query)) return 2
  if (normalized.startsWith(query)) return 3
  return 4
}

export function compareGraphEdges(left: GraphEdge, right: GraphEdge): number {
  return (
    left.source.localeCompare(right.source) ||
    left.target.localeCompare(right.target) ||
    left.resolver.localeCompare(right.resolver)
  )
}
