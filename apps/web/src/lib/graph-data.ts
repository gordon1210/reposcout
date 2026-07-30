import type {
  DependencyGraph,
  GraphEdge,
  GraphFile,
  GraphSymbol,
  GraphSymbolEdge,
  GraphSymbolReach,
} from "@/lib/types"

export const GRAPH_NODE_LIMIT = 100
export const GRAPH_SEARCH_LIMIT = 8

export type GraphDirection = "dependencies" | "dependents" | "both"

export interface GraphProjection {
  files: GraphFile[]
  edges: GraphEdge[]
  focus: string | null
  truncated: boolean
  totalFiles: number
}

export interface GraphResolverUsage {
  resolver: string
  connections: number
}

export interface GraphProminence {
  level: "standard" | "notable" | "hub"
  label: string
  reason: string
  basis: "none" | "dependency" | "symbol"
  reach: number
}

export interface GraphFileInspection {
  file: GraphFile
  incoming: GraphEdge[]
  outgoing: GraphEdge[]
  cycles: string[][]
  isOrphan: boolean
  roles: string[]
  resolverUsage: GraphResolverUsage[]
  prominence: GraphProminence
  symbols: GraphSymbol[]
  incomingSymbolEdges: GraphSymbolEdge[]
  outgoingSymbolEdges: GraphSymbolEdge[]
  symbolRelations: Array<{
    direction: "incoming" | "outgoing"
    relation: string
    symbol: GraphSymbol
  }>
}

export function projectGraph(
  graph: DependencyGraph,
  focus: string | null,
  direction: GraphDirection,
  depth: number,
  limit = GRAPH_NODE_LIMIT
): GraphProjection {
  const boundedLimit = Math.max(1, limit)
  const filesByPath = new Map(graph.files.map((file) => [file.path, file]))
  const validFocus = focus && filesByPath.has(focus) ? focus : null
  const edges = structuralFileEdges(graph)
  const selected = validFocus
    ? focusedPaths(
        edges,
        validFocus,
        direction,
        Math.max(1, depth),
        boundedLimit
      )
    : overviewPaths(graph.files, boundedLimit)
  const selectedPaths = selected.paths

  return {
    files: graph.files
      .filter((file) => selectedPaths.has(file.path))
      .sort((left, right) => left.path.localeCompare(right.path)),
    edges: edges
      .filter(
        (edge) =>
          selectedPaths.has(edge.source) && selectedPaths.has(edge.target)
      )
      .sort(
        (left, right) =>
          left.source.localeCompare(right.source) ||
          left.target.localeCompare(right.target) ||
          left.resolver.localeCompare(right.resolver)
      ),
    focus: validFocus,
    truncated: selected.truncated,
    totalFiles: graph.files.length,
  }
}

function overviewPaths(files: GraphFile[], limit: number): Selection {
  if (files.length <= limit) {
    return { paths: new Set(files.map((file) => file.path)), truncated: false }
  }
  const ranked = [...files].sort(
    (left, right) =>
      graphFileReach(right) - graphFileReach(left) ||
      right.fan_in - left.fan_in ||
      left.path.localeCompare(right.path)
  )
  return {
    paths: new Set(ranked.slice(0, limit).map((file) => file.path)),
    truncated: true,
  }
}

interface Selection {
  paths: Set<string>
  truncated: boolean
}

function focusedPaths(
  edges: GraphEdge[],
  focus: string,
  direction: GraphDirection,
  depth: number,
  limit: number
): Selection {
  const dependencies = new Map<string, string[]>()
  const dependents = new Map<string, string[]>()
  for (const edge of edges) {
    append(dependencies, edge.source, edge.target)
    append(dependents, edge.target, edge.source)
  }
  for (const neighbors of [...dependencies.values(), ...dependents.values()]) {
    neighbors.sort((left, right) => left.localeCompare(right))
  }

  const paths = new Set([focus])
  const queue: Array<[path: string, distance: number]> = [[focus, 0]]
  let truncated = false
  while (queue.length > 0) {
    const [path, distance] = queue.shift()!
    if (distance >= depth) continue
    const neighbors = graphNeighbors(path, direction, dependencies, dependents)
    for (const neighbor of neighbors) {
      if (paths.has(neighbor)) continue
      if (paths.size >= limit) {
        truncated = true
        continue
      }
      paths.add(neighbor)
      queue.push([neighbor, distance + 1])
    }
  }
  return { paths, truncated }
}

function append(map: Map<string, string[]>, key: string, value: string) {
  const values = map.get(key)
  if (values) values.push(value)
  else map.set(key, [value])
}

function graphNeighbors(
  path: string,
  direction: GraphDirection,
  dependencies: Map<string, string[]>,
  dependents: Map<string, string[]>
): string[] {
  if (direction === "dependencies") return dependencies.get(path) ?? []
  if (direction === "dependents") return dependents.get(path) ?? []
  return [
    ...new Set([
      ...(dependencies.get(path) ?? []),
      ...(dependents.get(path) ?? []),
    ]),
  ].sort((left, right) => left.localeCompare(right))
}

export function searchGraphFiles(
  graph: DependencyGraph,
  query: string,
  limit = GRAPH_SEARCH_LIMIT
): GraphFile[] {
  const normalized = query.trim().toLowerCase()
  if (!normalized) return []
  return graph.files
    .filter((file) => file.path.toLowerCase().includes(normalized))
    .sort(
      (left, right) =>
        matchRank(left.path, normalized) - matchRank(right.path, normalized) ||
        graphFileReach(right) - graphFileReach(left) ||
        left.path.localeCompare(right.path)
    )
    .slice(0, Math.max(1, limit))
}

export function inspectGraphFile(
  graph: DependencyGraph,
  path: string
): GraphFileInspection | null {
  const file = graph.files.find((candidate) => candidate.path === path)
  if (!file) return null

  const { incoming, outgoing } = fileEdges(graph, path)
  const cycles = graph.cycles.filter((cycle) => cycle.includes(path))
  const isOrphan = graph.orphans.includes(path)
  const prominence = graphProminence(file)
  const symbolInspection = inspectFileSymbols(graph, path)

  return {
    file,
    incoming,
    outgoing,
    cycles,
    isOrphan,
    roles: fileRoles(file, cycles, isOrphan, prominence),
    prominence,
    ...symbolInspection,
    resolverUsage: resolverUsage([...incoming, ...outgoing]),
  }
}

function fileEdges(
  graph: DependencyGraph,
  path: string
): Pick<GraphFileInspection, "incoming" | "outgoing"> {
  return {
    outgoing: graph.edge_list
      .filter((edge) => edge.source === path)
      .sort(compareGraphEdges),
    incoming: graph.edge_list
      .filter((edge) => edge.target === path)
      .sort(compareGraphEdges),
  }
}

function inspectFileSymbols(
  graph: DependencyGraph,
  path: string
): Pick<
  GraphFileInspection,
  "symbols" | "incomingSymbolEdges" | "outgoingSymbolEdges" | "symbolRelations"
> {
  const symbols = (graph.symbols ?? [])
    .filter((symbol) => symbol.path === path)
    .sort(
      (left, right) =>
        right.fan_in - left.fan_in ||
        right.fan_out - left.fan_out ||
        left.line - right.line
    )
  const symbolIds = new Set(symbols.map((symbol) => symbol.id))
  const incomingSymbolEdges = (graph.symbol_edges ?? []).filter((edge) =>
    symbolIds.has(edge.target)
  )
  const outgoingSymbolEdges = (graph.symbol_edges ?? []).filter((edge) =>
    symbolIds.has(edge.source)
  )
  const symbolsById = new Map(
    (graph.symbols ?? []).map((symbol) => [symbol.id, symbol])
  )
  const symbolRelations = [
    ...incomingSymbolEdges.map((edge) => ({
      direction: "incoming" as const,
      relation: edge.relation,
      symbol: symbolsById.get(edge.source),
    })),
    ...outgoingSymbolEdges.map((edge) => ({
      direction: "outgoing" as const,
      relation: edge.relation,
      symbol: symbolsById.get(edge.target),
    })),
  ]
    .filter(
      (
        entry
      ): entry is {
        direction: "incoming" | "outgoing"
        relation: string
        symbol: GraphSymbol
      } => Boolean(entry.symbol)
    )
    .sort(
      (left, right) =>
        left.direction.localeCompare(right.direction) ||
        left.relation.localeCompare(right.relation) ||
        left.symbol.qualified_name.localeCompare(right.symbol.qualified_name)
    )
  return {
    symbols,
    incomingSymbolEdges,
    outgoingSymbolEdges,
    symbolRelations,
  }
}

function fileRoles(
  file: GraphFile,
  cycles: string[][],
  isOrphan: boolean,
  prominence: GraphProminence
): string[] {
  const roles: string[] = []
  if (cycles.length > 0) roles.push("Cycle member")
  if (isOrphan) roles.push("Orphan candidate")
  if (prominence.level !== "standard") {
    roles.push(prominence.label)
  } else {
    if (file.fan_in >= 2) roles.push("Shared dependency")
    if (file.fan_out >= 2) roles.push("Coordinator")
  }
  if (roles.length === 0) roles.push(standardFileRole(file))
  return roles
}

function standardFileRole(file: GraphFile): string {
  if (file.fan_in > 0 && file.fan_out > 0) return "Connector"
  if (file.fan_in > 0) return "Leaf dependency"
  if (file.fan_out > 0) return "Top-level consumer"
  return "Isolated"
}

function resolverUsage(edges: GraphEdge[]): GraphResolverUsage[] {
  const resolverCounts = new Map<string, number>()
  const connections = new Map<string, GraphEdge>()
  for (const edge of edges) {
    connections.set(`${edge.source}\0${edge.target}\0${edge.resolver}`, edge)
  }
  for (const edge of connections.values()) {
    resolverCounts.set(
      edge.resolver,
      (resolverCounts.get(edge.resolver) ?? 0) + 1
    )
  }

  return [...resolverCounts.entries()]
    .map(([resolver, connections]) => ({ resolver, connections }))
    .sort(
      (left, right) =>
        right.connections - left.connections ||
        left.resolver.localeCompare(right.resolver)
    )
}

export function graphProminence(
  file:
    | (Pick<GraphFile, "fan_in" | "fan_out" | "symbol_reach"> & {
        language?: string
      })
    | null
): GraphProminence {
  if (!file) {
    return {
      level: "standard",
      label: "Standard",
      reason: "No resolved dependency node.",
      basis: "none",
      reach: 0,
    }
  }
  const symbol = file.symbol_reach
  if (symbol && symbol.fan_in > 0) return symbolProminence(symbol)
  const dependents = file.fan_in
  const dependencies = file.fan_out
  if (file.language?.toLowerCase() === "go" && dependents >= 2) {
    return goProminence(dependents, dependencies)
  }
  const rule = dependencyProminenceRules.find(({ matches }) =>
    matches(dependents, dependencies)
  )
  if (rule) return rule.build(dependents, dependencies)
  return {
    level: "standard",
    label: "Standard",
    reason: "Its resolved dependency reach is within the normal display tier.",
    basis: "dependency",
    reach: Math.max(dependents, dependencies),
  }
}

function goProminence(
  dependents: number,
  dependencies: number
): GraphProminence {
  const detail =
    dependencies > 0 ? `; it also imports ${dependencies} packages` : ""
  return {
    level: dependents >= 5 || dependencies >= 8 ? "hub" : "notable",
    label: "Package anchor",
    reason: `${dependents} resolved Go package imports are anchored at this representative file${detail}.`,
    basis: "dependency",
    reach: Math.max(dependents, dependencies),
  }
}

interface DependencyProminenceRule {
  matches: (dependents: number, dependencies: number) => boolean
  build: (dependents: number, dependencies: number) => GraphProminence
}

const dependencyProminenceRules: DependencyProminenceRule[] = [
  {
    matches: (dependents, dependencies) => dependents >= 5 && dependencies >= 4,
    build: (dependents, dependencies) => ({
      level: "hub",
      label: "Central hub",
      reason: `${dependents} files depend on it and it coordinates ${dependencies} dependencies.`,
      basis: "dependency",
      reach: Math.max(dependents, dependencies),
    }),
  },
  {
    matches: (dependents) => dependents >= 5,
    build: (dependents) => ({
      level: "hub",
      label: "High-impact dependency",
      reason: `${dependents} files directly depend on it through resolved relationships.`,
      basis: "dependency",
      reach: dependents,
    }),
  },
  {
    matches: (_dependents, dependencies) => dependencies >= 8,
    build: (_dependents, dependencies) => ({
      level: "hub",
      label: "Broad coordinator",
      reason: `It coordinates ${dependencies} resolved direct dependencies.`,
      basis: "dependency",
      reach: dependencies,
    }),
  },
  {
    matches: (dependents, dependencies) => dependents >= 2 && dependencies >= 4,
    build: (dependents, dependencies) => ({
      level: "notable",
      label: "Connector",
      reason: `${dependents} files depend on it and it coordinates ${dependencies} dependencies.`,
      basis: "dependency",
      reach: Math.max(dependents, dependencies),
    }),
  },
  {
    matches: (dependents) => dependents >= 2,
    build: (dependents) => ({
      level: "notable",
      label: "Shared dependency",
      reason: `${dependents} files directly depend on it through resolved relationships.`,
      basis: "dependency",
      reach: dependents,
    }),
  },
  {
    matches: (_dependents, dependencies) => dependencies >= 4,
    build: (_dependents, dependencies) => ({
      level: "notable",
      label: "Coordinator",
      reason: `It coordinates ${dependencies} resolved direct dependencies.`,
      basis: "dependency",
      reach: dependencies,
    }),
  },
]

function symbolProminence(symbol: GraphSymbolReach): GraphProminence {
  const level = symbol.fan_in >= 5 ? "hub" : "notable"
  const subject =
    symbol.kind === "interface" || symbol.kind === "trait"
      ? symbol.kind
      : symbol.kind === "class"
        ? "class"
        : "type"
  const label =
    symbol.relation === "implements"
      ? `Implemented ${subject}`
      : symbol.relation === "embeds"
        ? `Embedded ${subject}`
        : `Base ${subject}`
  const action =
    symbol.relation === "implements"
      ? "implement"
      : symbol.relation === "embeds"
        ? "embed"
        : "extend"
  return {
    level,
    label,
    reason: `${symbol.fan_in} declared ${symbol.fan_in === 1 ? "type" : "types"} directly ${action} ${symbol.name}.`,
    basis: "symbol",
    reach: symbol.fan_in,
  }
}

export function structuralFileEdges(graph: DependencyGraph): GraphEdge[] {
  const symbols = new Map(
    (graph.symbols ?? []).map((symbol) => [symbol.id, symbol])
  )
  const edges = new Map<string, GraphEdge>()
  for (const edge of graph.edge_list) {
    edges.set(`${edge.source}\0${edge.target}\0${edge.resolver}`, edge)
  }
  for (const edge of graph.symbol_edges ?? []) {
    const source = symbols.get(edge.source)?.path
    const target = symbols.get(edge.target)?.path
    if (!source || !target || source === target) continue
    const resolver = `symbol-${edge.relation}`
    edges.set(`${source}\0${target}\0${resolver}`, { source, target, resolver })
  }
  return [...edges.values()].sort(compareGraphEdges)
}

function graphFileReach(file: GraphFile): number {
  return Math.max(
    file.fan_in + file.fan_out,
    (file.symbol_reach?.fan_in ?? 0) * 2 + (file.symbol_reach?.fan_out ?? 0)
  )
}

function compareGraphEdges(left: GraphEdge, right: GraphEdge): number {
  return (
    left.source.localeCompare(right.source) ||
    left.target.localeCompare(right.target) ||
    left.resolver.localeCompare(right.resolver)
  )
}

function matchRank(path: string, query: string): number {
  const normalizedPath = path.toLowerCase()
  const name = graphFileName(normalizedPath)
  if (normalizedPath === query) return 0
  if (name === query) return 1
  if (name.startsWith(query)) return 2
  if (normalizedPath.startsWith(query)) return 3
  return 4
}

export function graphFileName(path: string): string {
  return path.split("/").at(-1) ?? path
}

export function graphParentPath(path: string): string {
  const parts = path.split("/")
  parts.pop()
  return parts.join("/") || "."
}
