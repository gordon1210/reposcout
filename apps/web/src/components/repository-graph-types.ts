import type { Edge, Node } from "@xyflow/react"

import type {
  ExplorerConnection,
  ExplorerEntity,
  ExplorerGroup,
} from "@/lib/graph-explorer-model"
import type { GraphProminence } from "@/lib/graph-data"
import type { GraphRoute } from "@/lib/graph-routes"

export interface GraphRouteNavigationOptions {
  replace?: boolean
}

export type NavigateGraphRoute = (
  route: GraphRoute,
  options?: GraphRouteNavigationOptions
) => void

export interface ExplorerNodeData extends Record<string, unknown> {
  entity: ExplorerEntity
  focused: boolean
  related: boolean
  dimmed: boolean
  vertical: boolean
  prominence: GraphProminence
  typeFocused: boolean
  width: number
  height: number
}

export type ExplorerNode = Node<ExplorerNodeData, "explorerEntity">

export interface ExplorerGroupNodeData extends Record<string, unknown> {
  group: ExplorerGroup
  related: boolean
  dimmed: boolean
}

export type ExplorerGroupFlowNode = Node<ExplorerGroupNodeData, "explorerGroup">
export type ExplorerFlowNode = ExplorerNode | ExplorerGroupFlowNode

export interface ExplorerEdgeData extends Record<string, unknown> {
  connection: ExplorerConnection
  showLabel: boolean
}

export type ExplorerEdge = Edge<ExplorerEdgeData, "explorerConnection">

export type GraphSelection =
  | { kind: "scope"; path: string }
  | { kind: "file"; path: string }
  | { kind: "group"; id: string }
  | { kind: "connection"; id: string }
  | null

export type ExplorerMode = "architecture" | "neighborhood"
