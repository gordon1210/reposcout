import { MarkerType, Position } from "@xyflow/react"

import {
  entityAriaLabel,
  relationLabel,
} from "@/components/repository-graph-visuals"
import type {
  ExplorerEdge,
  ExplorerFlowNode,
  ExplorerGroupFlowNode,
  ExplorerNode,
} from "@/components/repository-graph-types"
import type {
  ExplorerConnection,
  ExplorerGroup,
  ExplorerView,
} from "@/lib/graph-explorer-model"
import type {
  ExplorerGroupPlacement,
  ExplorerLayout,
} from "@/lib/graph-explorer-layout"

interface LayoutSelection {
  focus: string | null
  selectedEntityId: string | null
  selectedScopePath: string | null
  selectedGroupId: string | null
  selectedConnectionId: string | null
  hoveredEntityId: string | null
}

export interface DecoratedExplorerLayout {
  nodes: ExplorerFlowNode[]
  edges: ExplorerEdge[]
}

export function decorateLayout(
  view: ExplorerView,
  placements: ExplorerLayout,
  selection: LayoutSelection
): DecoratedExplorerLayout {
  const relatedIds = collectRelatedIds(view, placements, selection)
  const focusedId = focusedEntityId(view, selection.focus)
  const connectedEdgeCount = countConnectedEdges(
    view,
    selection.selectedEntityId
  )
  const groups = placements.groups.map((placement) =>
    decorateGroup(placement, relatedIds, selection)
  )
  const entities = view.entities.map((entity) =>
    decorateEntity(
      entity,
      placements,
      view.presentation,
      relatedIds,
      focusedId,
      selection
    )
  )
  const edges = view.connections.map((connection) =>
    decorateConnection(connection, view, placements, {
      ...selection,
      relatedIds,
      focusedId,
      connectedEdgeCount,
    })
  )
  return { nodes: [...groups, ...entities], edges }
}

function collectRelatedIds(
  view: ExplorerView,
  placements: ExplorerLayout,
  selection: LayoutSelection
): Set<string> {
  const related = new Set<string>()
  addSelectedEntityRelations(view, selection.selectedEntityId, related)
  addSelectedConnectionRelations(view, selection.selectedConnectionId, related)
  const group = findSelectedGroup(placements, selection)
  if (group) addSelectedGroupRelations(view, group.group, related)
  return related
}

function addSelectedEntityRelations(
  view: ExplorerView,
  selectedId: string | null,
  related: Set<string>
) {
  if (!selectedId) return
  related.add(selectedId)
  for (const connection of view.connections) {
    if (!connectionTouches(connection, selectedId)) continue
    related.add(connection.source)
    related.add(connection.target)
  }
}

function addSelectedConnectionRelations(
  view: ExplorerView,
  selectedId: string | null,
  related: Set<string>
) {
  if (!selectedId) return
  const connection = view.connections.find(
    (candidate) => candidate.id === selectedId
  )
  if (!connection) return
  related.add(connection.source)
  related.add(connection.target)
}

function findSelectedGroup(
  placements: ExplorerLayout,
  selection: LayoutSelection
): ExplorerGroupPlacement | undefined {
  return placements.groups.find(({ group }) => {
    if (selection.selectedGroupId) {
      return group.id === selection.selectedGroupId
    }
    return Boolean(
      selection.selectedScopePath && group.path === selection.selectedScopePath
    )
  })
}

function addSelectedGroupRelations(
  view: ExplorerView,
  group: ExplorerGroup,
  related: Set<string>
) {
  const members = new Set(group.members.map((member) => member.id))
  for (const id of members) related.add(id)
  for (const connection of view.connections) {
    if (!members.has(connection.source) && !members.has(connection.target)) {
      continue
    }
    related.add(connection.source)
    related.add(connection.target)
  }
}

function focusedEntityId(
  view: ExplorerView,
  focus: string | null
): string | null {
  if (!focus) return null
  return (
    view.entities.find(
      (entity) => entity.kind === "file" && entity.path === focus
    )?.id ?? null
  )
}

function countConnectedEdges(
  view: ExplorerView,
  selectedId: string | null
): number {
  if (!selectedId) return 0
  return view.connections.filter((connection) =>
    connectionTouches(connection, selectedId)
  ).length
}

function connectionTouches(
  connection: ExplorerConnection,
  entityId: string
): boolean {
  return connection.source === entityId || connection.target === entityId
}

function decorateGroup(
  { group, position, width, height }: ExplorerGroupPlacement,
  relatedIds: Set<string>,
  selection: LayoutSelection
): ExplorerGroupFlowNode {
  const related = group.members.some((member) => relatedIds.has(member.id))
  return {
    id: group.id,
    type: "explorerGroup",
    position,
    width,
    height,
    style: { width, height },
    selected:
      selection.selectedGroupId === group.id ||
      selection.selectedScopePath === group.path,
    selectable: true,
    zIndex: 0,
    ariaLabel: groupAriaLabel(group),
    data: {
      group,
      related,
      dimmed: relatedIds.size > 0 && !related,
    },
  }
}

function groupAriaLabel(group: ExplorerGroup): string {
  if (group.kind === "relationship") {
    const total = group.totalMembers ?? group.members.length
    return `${group.label} ${group.name}, ${group.members.length} of ${total} files visible`
  }
  return `${group.label} ${group.path || "Project root"}, ${group.members.length} visible items`
}

function decorateEntity(
  entity: ExplorerView["entities"][number],
  placements: ExplorerLayout,
  mode: ExplorerView["presentation"],
  relatedIds: Set<string>,
  focusedId: string | null,
  selection: LayoutSelection
): ExplorerNode {
  const placement = placements.entities.get(entity.id)!
  const prominence = placements.prominence.get(entity.id)!
  const parented = Boolean(placement.parentId)
  return {
    id: entity.id,
    type: "explorerEntity",
    position: placement.position,
    width: placement.width,
    height: placement.height,
    parentId: placement.parentId,
    extent: parented ? "parent" : undefined,
    zIndex: parented ? 1 : 0,
    sourcePosition: placements.vertical ? Position.Bottom : Position.Right,
    targetPosition: placements.vertical ? Position.Top : Position.Left,
    selected: selection.selectedEntityId === entity.id,
    ariaLabel: entityAriaLabel(entity, prominence),
    data: {
      entity,
      focused: focusedId === entity.id,
      related: relatedIds.has(entity.id),
      dimmed: relatedIds.size > 0 && !relatedIds.has(entity.id),
      vertical: placements.vertical,
      prominence,
      typeFocused: viewTypeIsFocused(mode, entity.path, selection.focus),
      width: placement.width,
      height: placement.height,
    },
  }
}

function viewTypeIsFocused(
  mode: ExplorerView["presentation"],
  path: string,
  focus: string | null
): boolean {
  return mode === "type" && path === focus
}

interface ConnectionDecoration extends LayoutSelection {
  relatedIds: Set<string>
  focusedId: string | null
  connectedEdgeCount: number
}

function decorateConnection(
  connection: ExplorerConnection,
  view: ExplorerView,
  placements: ExplorerLayout,
  selection: ConnectionDecoration
): ExplorerEdge {
  const state = connectionState(connection, view.presentation, selection)
  const baseWidth =
    1.1 + Math.min(2.4, Math.log2(Math.max(1, connection.count)) * 0.7)
  return {
    id: connection.id,
    source: connection.source,
    target: connection.target,
    type: "explorerConnection",
    selected: state.selected,
    ariaLabel: `${relationLabel(connection.relation)} from ${connection.source} to ${connection.target}, ${connection.count} file connections`,
    markerEnd:
      state.highlighted || !placements.dense
        ? {
            type: MarkerType.ArrowClosed,
            color: state.color,
            width: 15,
            height: 15,
          }
        : undefined,
    style: connectionStyle(baseWidth, placements, view.presentation, state),
    zIndex: state.selected ? 3 : state.highlighted ? 2 : 0,
    data: {
      connection,
      showLabel: shouldShowConnectionLabel(
        connection,
        view,
        placements,
        selection,
        state
      ),
    },
  }
}

interface ConnectionState {
  selected: boolean
  connected: boolean
  hovered: boolean
  highlighted: boolean
  muted: boolean
  semanticImport: boolean
  color: string
}

function connectionState(
  connection: ExplorerConnection,
  mode: ExplorerView["presentation"],
  selection: ConnectionDecoration
): ConnectionState {
  const selected = selection.selectedConnectionId === connection.id
  const connected = Boolean(
    selection.selectedEntityId &&
    connectionTouches(connection, selection.selectedEntityId)
  )
  const focused = Boolean(
    selection.focusedId && connectionTouches(connection, selection.focusedId)
  )
  const hovered = Boolean(
    selection.hoveredEntityId &&
    connectionTouches(connection, selection.hoveredEntityId)
  )
  const highlighted = selected || connected || focused || hovered
  const explicitType = ["extends", "implements", "embeds"].includes(
    connection.relation
  )
  const semanticImport = mode === "type" && !explicitType
  return {
    selected,
    connected,
    hovered,
    highlighted,
    muted: selection.relatedIds.size > 0 && !selected && !connected && !hovered,
    semanticImport,
    color: connectionColor(selected, highlighted, explicitType, mode),
  }
}

function connectionColor(
  selected: boolean,
  highlighted: boolean,
  explicitType: boolean,
  mode: ExplorerView["presentation"]
): string {
  if (selected) return "var(--chart-3)"
  if (mode === "type" && explicitType) return "var(--primary)"
  return highlighted ? "var(--foreground)" : "var(--muted-foreground)"
}

function connectionStyle(
  baseWidth: number,
  placements: ExplorerLayout,
  mode: ExplorerView["presentation"],
  state: ConnectionState
) {
  return {
    stroke: state.color,
    strokeWidth: connectionWidth(baseWidth, placements.dense, state),
    strokeDasharray: state.semanticImport ? "7 5" : undefined,
    opacity: connectionOpacity(placements, mode, state),
  }
}

function connectionWidth(
  baseWidth: number,
  dense: boolean,
  state: ConnectionState
): number {
  if (state.selected) return Math.max(2.8, baseWidth)
  if (state.highlighted) return Math.max(2, baseWidth)
  return dense ? Math.min(1, baseWidth) : baseWidth
}

function connectionOpacity(
  placements: ExplorerLayout,
  mode: ExplorerView["presentation"],
  state: ConnectionState
): number {
  if (state.semanticImport) {
    return state.selected || state.hovered ? 0.85 : 0.42
  }
  if (state.muted) return placements.dense ? 0.025 : 0.06
  if (state.highlighted) return 0.94
  if (placements.dense) return 0.1
  return mode === "architecture" ? 0.42 : 0.24
}

function shouldShowConnectionLabel(
  connection: ExplorerConnection,
  view: ExplorerView,
  placements: ExplorerLayout,
  selection: ConnectionDecoration,
  state: ConnectionState
): boolean {
  const idleAggregate =
    !placements.dense &&
    view.presentation === "architecture" &&
    view.connections.length <= 12 &&
    connection.count > 1
  const selectedEntityConnection =
    Boolean(selection.selectedEntityId) &&
    state.connected &&
    selection.connectedEdgeCount <= 6
  return state.selected || idleAggregate || selectedEntityConnection
}
