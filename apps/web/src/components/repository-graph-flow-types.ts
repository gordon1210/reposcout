import {
  ExplorerConnectionEdge,
  ExplorerEntityNode,
  ExplorerGroupNode,
} from "@/components/repository-graph-canvas"
import type { ExplorerNodeData } from "@/components/repository-graph-types"
import { miniMapEntityColor } from "@/components/repository-graph-visuals"

export const nodeTypes = {
  explorerEntity: ExplorerEntityNode,
  explorerGroup: ExplorerGroupNode,
}

export const edgeTypes = { explorerConnection: ExplorerConnectionEdge }

export function miniMapNodeColor(data: Record<string, unknown>): string {
  if ("group" in data) return "#334155"
  if (isExplorerNodeData(data)) return miniMapEntityColor(data.entity)
  return "#64748b"
}

function isExplorerNodeData(
  data: Record<string, unknown>
): data is ExplorerNodeData {
  return (
    "entity" in data && typeof data.entity === "object" && data.entity !== null
  )
}
