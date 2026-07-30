import dagre, {
  type EdgeLabel,
  type Graph,
  type GraphLabel,
  type NodeLabel,
  type Point,
} from "@dagrejs/dagre"

import {
  EXPLORER_NODE_LIMIT,
  groupExplorerFiles,
  type ExplorerEntity,
  type ExplorerGroup,
  type ExplorerView,
} from "@/lib/graph-explorer-model"
import { graphProminence, type GraphProminence } from "@/lib/graph-data"

type DagreGraph = Graph<GraphLabel, NodeLabel, EdgeLabel>

const SCOPE_NODE_WIDTH = 284
const SCOPE_NODE_HEIGHT = 126
const FILE_NODE_WIDTH = 244
const FILE_NODE_HEIGHT = 92
const NOTABLE_FILE_NODE_WIDTH = 270
const NOTABLE_FILE_NODE_HEIGHT = 104
const HUB_FILE_NODE_WIDTH = 304
const HUB_FILE_NODE_HEIGHT = 116
const GROUP_HEADER_HEIGHT = 48
const GROUP_PADDING = 22

export type ExplorerLayoutMode = "architecture" | "neighborhood" | "type"

export interface ExplorerEntityPlacement {
  position: { x: number; y: number }
  width: number
  height: number
  parentId?: string
}

export interface ExplorerGroupPlacement {
  group: ExplorerGroup
  position: { x: number; y: number }
  width: number
  height: number
}

export interface ExplorerLayout {
  entities: Map<string, ExplorerEntityPlacement>
  groups: ExplorerGroupPlacement[]
  prominence: Map<string, GraphProminence>
  vertical: boolean
  dense: boolean
}

export function layoutExplorerView(
  view: ExplorerView,
  mode: ExplorerLayoutMode
): ExplorerLayout {
  const dense = mode === "type" ? false : isDenseExplorerView(view)
  const vertical =
    mode === "architecture" &&
    view.entities.some((entity) => entity.kind === "scope")
  const prominence = new Map(
    view.entities.map((entity) => [
      entity.id,
      graphProminence(entity.kind === "file" ? entity.graphFile : null),
    ])
  )
  const groups =
    mode === "architecture" || mode === "type"
      ? (view.groups ?? [])
      : groupExplorerFiles(view.entities)
  const placements =
    mode === "type" && groups.length > 0
      ? layoutTypeNeighborhood(view, groups, prominence)
      : shouldGroupFiles(mode, view.entities.length, groups)
        ? layoutGroupedFiles(view, groups, prominence, dense)
        : layoutFlatEntities(view, prominence, vertical, dense)

  return { ...placements, prominence, vertical, dense }
}

export function isDenseExplorerView(view: ExplorerView): boolean {
  return (
    view.connections.length >= 18 ||
    (view.entities.length >= 12 &&
      view.connections.length > view.entities.length * 1.25)
  )
}

function entityDimensions(
  entity: ExplorerEntity,
  prominence: GraphProminence
): [number, number] {
  if (entity.kind === "scope") return [SCOPE_NODE_WIDTH, SCOPE_NODE_HEIGHT]
  if (prominence.basis === "symbol" && prominence.level === "hub") {
    const scale = Math.sqrt(Math.max(1, prominence.reach))
    return [
      Math.min(430, Math.round(320 + scale * 18)),
      Math.min(158, Math.round(118 + scale * 7)),
    ]
  }
  if (prominence.basis === "symbol" && prominence.level === "notable") {
    return [292, 112]
  }
  if (prominence.level === "hub")
    return [HUB_FILE_NODE_WIDTH, HUB_FILE_NODE_HEIGHT]
  if (prominence.level === "notable")
    return [NOTABLE_FILE_NODE_WIDTH, NOTABLE_FILE_NODE_HEIGHT]
  return [FILE_NODE_WIDTH, FILE_NODE_HEIGHT]
}

function layoutFlatEntities(
  view: ExplorerView,
  prominence: Map<string, GraphProminence>,
  vertical: boolean,
  dense: boolean
): Pick<ExplorerLayout, "entities" | "groups"> {
  const layout = new dagre.graphlib.Graph()
  layout.setDefaultEdgeLabel(() => ({}))
  layout.setGraph({
    rankdir: vertical ? "TB" : "LR",
    ranksep: vertical ? (dense ? 118 : 86) : dense ? 148 : 104,
    nodesep: dense ? 52 : vertical ? 38 : 30,
    edgesep: dense ? 38 : 24,
    marginx: 34,
    marginy: 34,
    acyclicer: "greedy",
    ranker: "network-simplex",
  })
  for (const entity of view.entities) {
    const [width, height] = entityDimensions(entity, prominence.get(entity.id)!)
    layout.setNode(entity.id, { width, height })
  }
  for (const connection of view.connections)
    layout.setEdge(connection.source, connection.target)
  dagre.layout(layout)

  return {
    groups: [],
    entities: new Map(
      view.entities.map((entity) => {
        const point = layout.node(entity.id)
        const [width, height] = entityDimensions(
          entity,
          prominence.get(entity.id)!
        )
        return [
          entity.id,
          {
            position: { x: point.x - width / 2, y: point.y - height / 2 },
            width,
            height,
          },
        ]
      })
    ),
  }
}

function shouldGroupFiles(
  mode: ExplorerLayoutMode,
  entityCount: number,
  groups: ExplorerGroup[]
): boolean {
  if (mode === "architecture") {
    return (
      groups.length > 0 && entityCount + groups.length <= EXPLORER_NODE_LIMIT
    )
  }
  if (mode === "type") return groups.length > 0
  return (
    mode === "neighborhood" &&
    entityCount >= 4 &&
    groups.length >= 2 &&
    entityCount + groups.length <= EXPLORER_NODE_LIMIT &&
    groups.length <= Math.min(12, Math.floor(entityCount / 2)) &&
    groups.some((group) => group.members.length >= 2)
  )
}

function layoutTypeNeighborhood(
  view: ExplorerView,
  groups: ExplorerGroup[],
  prominence: Map<string, GraphProminence>
): Pick<ExplorerLayout, "entities" | "groups"> {
  const focus = view.entities.find(
    (entity) => entity.kind === "file" && entity.path === view.focusPath
  )
  if (!focus) return layoutFlatEntities(view, prominence, false, false)

  const dimensions = new Map<string, { width: number; height: number }>()
  const entities = new Map<string, ExplorerEntityPlacement>()
  for (const group of groups) {
    const grid = layoutGridGroup(
      group,
      prominence,
      group.relationship?.family === "type"
        ? group.members.length > 12
          ? 4
          : 2
        : 3
    )
    dimensions.set(group.id, { width: grid.width, height: grid.height })
    for (const [id, placement] of grid.entities) entities.set(id, placement)
  }

  const incoming = groups.filter(
    (group) => group.relationship?.direction === "incoming"
  )
  const outgoing = groups.filter(
    (group) => group.relationship?.direction === "outgoing"
  )
  const stackGap = 52
  const incomingHeight = stackHeight(incoming, dimensions, stackGap)
  const outgoingHeight = stackHeight(outgoing, dimensions, stackGap)
  const incomingWidth = maximumGroupWidth(incoming, dimensions)
  const [ordinaryFocusWidth, ordinaryFocusHeight] = entityDimensions(
    focus,
    prominence.get(focus.id)!
  )
  const focusWidth = Math.max(480, ordinaryFocusWidth)
  const focusHeight = Math.max(176, ordinaryFocusHeight)
  const margin = 40
  const horizontalGap = 180
  const canvasHeight =
    Math.max(360, incomingHeight, outgoingHeight, focusHeight) + margin * 2
  const focusX =
    margin + (incoming.length > 0 ? incomingWidth + horizontalGap : 0)
  const focusY = (canvasHeight - focusHeight) / 2
  entities.set(focus.id, {
    position: { x: focusX, y: focusY },
    width: focusWidth,
    height: focusHeight,
  })

  const placements: ExplorerGroupPlacement[] = []
  placeGroupStack(
    incoming,
    margin,
    (canvasHeight - incomingHeight) / 2,
    dimensions,
    stackGap,
    placements
  )
  placeGroupStack(
    outgoing,
    focusX + focusWidth + horizontalGap,
    (canvasHeight - outgoingHeight) / 2,
    dimensions,
    stackGap,
    placements
  )

  const groupedIds = new Set(
    groups.flatMap((group) => group.members.map((member) => member.id))
  )
  const ungrouped = view.entities.filter(
    (entity) => entity.id !== focus.id && !groupedIds.has(entity.id)
  )
  for (const [index, entity] of ungrouped.entries()) {
    const [width, height] = entityDimensions(entity, prominence.get(entity.id)!)
    entities.set(entity.id, {
      position: {
        x: focusX + index * (width + 36),
        y: focusY + focusHeight + 72,
      },
      width,
      height,
    })
  }

  return { entities, groups: placements }
}

function stackHeight(
  groups: ExplorerGroup[],
  dimensions: Map<string, { width: number; height: number }>,
  gap: number
): number {
  if (groups.length === 0) return 0
  return (
    groups.reduce(
      (total, group) => total + dimensions.get(group.id)!.height,
      0
    ) +
    gap * (groups.length - 1)
  )
}

function maximumGroupWidth(
  groups: ExplorerGroup[],
  dimensions: Map<string, { width: number; height: number }>
): number {
  return groups.reduce(
    (maximum, group) => Math.max(maximum, dimensions.get(group.id)!.width),
    0
  )
}

function placeGroupStack(
  groups: ExplorerGroup[],
  x: number,
  initialY: number,
  dimensions: Map<string, { width: number; height: number }>,
  gap: number,
  placements: ExplorerGroupPlacement[]
) {
  let y = initialY
  for (const group of groups) {
    const { width, height } = dimensions.get(group.id)!
    placements.push({ group, position: { x, y }, width, height })
    y += height + gap
  }
}

function layoutGroupedFiles(
  view: ExplorerView,
  groups: ExplorerGroup[],
  prominence: Map<string, GraphProminence>,
  dense: boolean
): Pick<ExplorerLayout, "entities" | "groups"> {
  const groupByEntity = new Map<string, ExplorerGroup>()
  const localPositions = new Map<string, ExplorerEntityPlacement>()
  const dimensions = new Map<string, { width: number; height: number }>()

  for (const group of groups) {
    for (const member of group.members) {
      groupByEntity.set(member.id, group)
    }
    const local = layoutLocalGroup(view, group, prominence, dense)
    dimensions.set(group.id, { width: local.width, height: local.height })
    for (const [id, placement] of local.entities) {
      localPositions.set(id, placement)
    }
  }

  return layoutOuterGroups(
    view,
    groups,
    prominence,
    dense,
    groupByEntity,
    localPositions,
    dimensions
  )
}

function layoutLocalGroup(
  view: ExplorerView,
  group: ExplorerGroup,
  prominence: Map<string, GraphProminence>,
  dense: boolean
): {
  width: number
  height: number
  entities: Map<string, ExplorerEntityPlacement>
} {
  if (group.kind === "architecture") {
    return layoutGridGroup(group, prominence, 4)
  }
  return layoutDagreGroup(view, group, prominence, dense)
}

function layoutDagreGroup(
  view: ExplorerView,
  group: ExplorerGroup,
  prominence: Map<string, GraphProminence>,
  dense: boolean
): {
  width: number
  height: number
  entities: Map<string, ExplorerEntityPlacement>
} {
  const local = new dagre.graphlib.Graph()
  local.setDefaultEdgeLabel(() => ({}))
  local.setGraph({
    rankdir: "LR",
    ranksep: dense ? 90 : 72,
    nodesep: dense ? 36 : 28,
    edgesep: 18,
    marginx: 0,
    marginy: 0,
    acyclicer: "greedy",
    ranker: "network-simplex",
  })
  const memberIds = new Set(group.members.map((member) => member.id))
  for (const member of group.members) {
    const [width, height] = entityDimensions(member, prominence.get(member.id)!)
    local.setNode(member.id, { width, height })
  }
  for (const connection of view.connections) {
    if (memberIds.has(connection.source) && memberIds.has(connection.target)) {
      local.setEdge(connection.source, connection.target)
    }
  }
  dagre.layout(local)

  const bounds = localGroupBounds(local, group, prominence)
  return {
    width: Math.max(340, bounds.maxX - bounds.minX + GROUP_PADDING * 2),
    height: Math.max(
      190,
      bounds.maxY - bounds.minY + GROUP_HEADER_HEIGHT + GROUP_PADDING
    ),
    entities: localGroupPlacements(local, group, prominence, bounds),
  }
}

interface GroupBounds {
  minX: number
  minY: number
  maxX: number
  maxY: number
}

function localGroupBounds(
  graph: DagreGraph,
  group: ExplorerGroup,
  prominence: Map<string, GraphProminence>
): GroupBounds {
  const bounds: GroupBounds = {
    minX: Number.POSITIVE_INFINITY,
    minY: Number.POSITIVE_INFINITY,
    maxX: Number.NEGATIVE_INFINITY,
    maxY: Number.NEGATIVE_INFINITY,
  }
  for (const member of group.members) {
    const point = nodePoint(graph, member.id)
    const [width, height] = entityDimensions(member, prominence.get(member.id)!)
    bounds.minX = Math.min(bounds.minX, point.x - width / 2)
    bounds.minY = Math.min(bounds.minY, point.y - height / 2)
    bounds.maxX = Math.max(bounds.maxX, point.x + width / 2)
    bounds.maxY = Math.max(bounds.maxY, point.y + height / 2)
  }
  return bounds
}

function localGroupPlacements(
  graph: DagreGraph,
  group: ExplorerGroup,
  prominence: Map<string, GraphProminence>,
  bounds: GroupBounds
): Map<string, ExplorerEntityPlacement> {
  const placements = new Map<string, ExplorerEntityPlacement>()
  for (const member of group.members) {
    const point = nodePoint(graph, member.id)
    const [width, height] = entityDimensions(member, prominence.get(member.id)!)
    placements.set(member.id, {
      position: {
        x: point.x - width / 2 - bounds.minX + GROUP_PADDING,
        y: point.y - height / 2 - bounds.minY + GROUP_HEADER_HEIGHT,
      },
      width,
      height,
      parentId: group.id,
    })
  }
  return placements
}

function layoutOuterGroups(
  view: ExplorerView,
  groups: ExplorerGroup[],
  prominence: Map<string, GraphProminence>,
  dense: boolean,
  groupByEntity: Map<string, ExplorerGroup>,
  localPositions: Map<string, ExplorerEntityPlacement>,
  dimensions: Map<string, { width: number; height: number }>
): Pick<ExplorerLayout, "entities" | "groups"> {
  const outer = createOuterGraph(dense)
  for (const group of groups) {
    outer.setNode(group.id, dimensions.get(group.id)!)
  }
  const ungrouped = view.entities.filter(
    (entity) => !groupByEntity.has(entity.id)
  )
  for (const entity of ungrouped) {
    const [width, height] = entityDimensions(entity, prominence.get(entity.id)!)
    outer.setNode(entity.id, { width, height })
  }
  addOuterConnections(outer, view.connections, groupByEntity)
  dagre.layout(outer)

  placeUngroupedEntities(outer, ungrouped, prominence, localPositions)

  return {
    entities: localPositions,
    groups: groups.map((group) =>
      placeOuterGroup(outer, group, dimensions.get(group.id)!)
    ),
  }
}

function createOuterGraph(dense: boolean): DagreGraph {
  const graph = new dagre.graphlib.Graph()
  graph.setDefaultEdgeLabel(() => ({}))
  graph.setGraph({
    rankdir: "LR",
    ranksep: dense ? 180 : 140,
    nodesep: dense ? 96 : 72,
    edgesep: 32,
    marginx: 40,
    marginy: 40,
    acyclicer: "greedy",
    ranker: "network-simplex",
  })
  return graph
}

function addOuterConnections(
  graph: DagreGraph,
  connections: ExplorerView["connections"],
  groupByEntity: Map<string, ExplorerGroup>
) {
  for (const connection of connections) {
    const source = groupByEntity.get(connection.source)?.id ?? connection.source
    const target = groupByEntity.get(connection.target)?.id ?? connection.target
    if (source !== target) graph.setEdge(source, target)
  }
}

function placeUngroupedEntities(
  graph: DagreGraph,
  entities: ExplorerEntity[],
  prominence: Map<string, GraphProminence>,
  placements: Map<string, ExplorerEntityPlacement>
) {
  for (const entity of entities) {
    const point = nodePoint(graph, entity.id)
    const [width, height] = entityDimensions(entity, prominence.get(entity.id)!)
    placements.set(entity.id, {
      position: { x: point.x - width / 2, y: point.y - height / 2 },
      width,
      height,
    })
  }
}

function placeOuterGroup(
  graph: DagreGraph,
  group: ExplorerGroup,
  dimensions: { width: number; height: number }
): ExplorerGroupPlacement {
  const point = nodePoint(graph, group.id)
  return {
    group,
    position: {
      x: point.x - dimensions.width / 2,
      y: point.y - dimensions.height / 2,
    },
    ...dimensions,
  }
}

function nodePoint(graph: DagreGraph, id: string): Point {
  const node = graph.node(id)
  if (typeof node.x !== "number" || typeof node.y !== "number") {
    throw new Error(`Dagre did not position graph node ${id}`)
  }
  return { x: node.x, y: node.y }
}

function layoutGridGroup(
  group: ExplorerGroup,
  prominence: Map<string, GraphProminence>,
  maximumColumns: number
): {
  width: number
  height: number
  entities: Map<string, ExplorerEntityPlacement>
} {
  const members = [...group.members].sort((left, right) =>
    left.path.localeCompare(right.path)
  )
  const columns = Math.min(
    maximumColumns,
    Math.max(1, Math.ceil(Math.sqrt(members.length * 1.5)))
  )
  const rows = Math.ceil(members.length / columns)
  const columnWidths = Array.from({ length: columns }, () => 0)
  const rowHeights = Array.from({ length: rows }, () => 0)
  const sizes = members.map((member, index) => {
    const [width, height] = entityDimensions(member, prominence.get(member.id)!)
    const column = index % columns
    const row = Math.floor(index / columns)
    columnWidths[column] = Math.max(columnWidths[column], width)
    rowHeights[row] = Math.max(rowHeights[row], height)
    return { member, width, height, column, row }
  })
  const columnGap = 42
  const rowGap = 34
  const columnOffsets = columnWidths.map(
    (_, index) =>
      GROUP_PADDING +
      columnWidths.slice(0, index).reduce((total, width) => total + width, 0) +
      columnGap * index
  )
  const rowOffsets = rowHeights.map(
    (_, index) =>
      GROUP_HEADER_HEIGHT +
      rowHeights.slice(0, index).reduce((total, height) => total + height, 0) +
      rowGap * index
  )
  const entities = new Map<string, ExplorerEntityPlacement>()
  for (const { member, width, height, column, row } of sizes) {
    entities.set(member.id, {
      position: {
        x: columnOffsets[column] + (columnWidths[column] - width) / 2,
        y: rowOffsets[row] + (rowHeights[row] - height) / 2,
      },
      width,
      height,
      parentId: group.id,
    })
  }
  return {
    width: Math.max(
      420,
      columnWidths.reduce((total, width) => total + width, 0) +
        columnGap * (columns - 1) +
        GROUP_PADDING * 2
    ),
    height: Math.max(
      210,
      rowHeights.reduce((total, height) => total + height, 0) +
        rowGap * (rows - 1) +
        GROUP_HEADER_HEIGHT +
        GROUP_PADDING
    ),
    entities,
  }
}
