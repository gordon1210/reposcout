import type {
  DependencyGraph,
  GraphEdge,
  GraphSymbol,
  GraphSymbolEdge,
} from "@/lib/types"

export type TypeNeighborhoodFamily = "type" | "import"
export type TypeNeighborhoodDirection = "incoming" | "outgoing"
type TypeNeighborhoodRelation = "extends" | "implements" | "embeds"

export interface TypeNeighborhoodGroup {
  id: string
  label: string
  name: string
  family: TypeNeighborhoodFamily
  direction: TypeNeighborhoodDirection
  relation: TypeNeighborhoodRelation | "imports"
  description: string
  paths: string[]
  totalMembers: number
}

export interface TypeNeighborhoodProjection {
  focus: string
  symbol: GraphSymbol
  files: string[]
  edges: GraphEdge[]
  groups: TypeNeighborhoodGroup[]
  totalFiles: number
  truncated: boolean
}

interface MutableGroup extends Omit<
  TypeNeighborhoodGroup,
  "paths" | "totalMembers"
> {
  paths: string[]
}

const RELATION_ORDER = new Map([
  ["extends", 0],
  ["implements", 1],
  ["embeds", 2],
])
const IMPORT_CONTEXT_LIMIT = 12

export function projectTypeNeighborhood(
  graph: DependencyGraph,
  focus: string,
  limit = 100
): TypeNeighborhoodProjection | null {
  const focusFile = graph.files.find((file) => file.path === focus)
  if (!focusFile) return null

  const symbols = graph.symbols ?? []
  const symbolEdges = graph.symbol_edges ?? []
  const symbolsById = new Map(symbols.map((symbol) => [symbol.id, symbol]))
  const symbol = selectFocusSymbol(
    symbols,
    symbolEdges,
    focus,
    focusFile.symbol_reach?.symbol_id
  )
  if (!symbol) return null

  const { groups: rawGroups, edges: typeEdges } = collectTypeGroups(
    symbolEdges,
    symbolsById,
    focus,
    symbol
  )
  if (rawGroups.size === 0) return null

  const typePaths = new Set(
    [...rawGroups.values()].flatMap((group) => group.paths)
  )
  const importEdges = collectImportGroups(
    graph.edge_list,
    rawGroups,
    typePaths,
    focus,
    symbol
  )
  const { groups: distinct, claimed } = distinctGroups(rawGroups)

  const totalFiles = 1 + claimed.size
  const groups = retainGroups(distinct, Math.max(1, limit))
  const retainedPaths = new Set([
    focus,
    ...groups.flatMap((group) => group.paths),
  ])
  const edges = dedupeEdges([...typeEdges, ...importEdges]).filter(
    (edge) => retainedPaths.has(edge.source) && retainedPaths.has(edge.target)
  )

  return {
    focus,
    symbol,
    files: [...retainedPaths].sort((left, right) =>
      left === focus ? -1 : right === focus ? 1 : left.localeCompare(right)
    ),
    edges,
    groups,
    totalFiles,
    truncated: retainedPaths.size < totalFiles,
  }
}

function collectTypeGroups(
  edges: GraphSymbolEdge[],
  symbolsById: Map<string, GraphSymbol>,
  focus: string,
  symbol: GraphSymbol
): { groups: Map<string, MutableGroup>; edges: GraphEdge[] } {
  const groups = new Map<string, MutableGroup>()
  const fileEdges: GraphEdge[] = []
  for (const edge of edges) {
    if (!isTypeRelation(edge.relation)) continue
    const direction = symbolEdgeDirection(edge, symbol.id)
    if (!direction) continue
    const related = symbolsById.get(relatedSymbolId(edge, direction))
    if (!related || related.path === focus) continue
    appendGroup(
      groups,
      relationshipGroup(focus, symbol, edge.relation, direction),
      related.path
    )
    fileEdges.push(typeFileEdge(focus, related.path, edge.relation, direction))
  }
  return { groups, edges: fileEdges }
}

function symbolEdgeDirection(
  edge: GraphSymbolEdge,
  symbolId: string
): TypeNeighborhoodDirection | null {
  if (edge.target === symbolId) return "incoming"
  if (edge.source === symbolId) return "outgoing"
  return null
}

function relatedSymbolId(
  edge: GraphSymbolEdge,
  direction: TypeNeighborhoodDirection
): string {
  return direction === "incoming" ? edge.source : edge.target
}

function typeFileEdge(
  focus: string,
  relatedPath: string,
  relation: TypeNeighborhoodRelation,
  direction: TypeNeighborhoodDirection
): GraphEdge {
  return {
    source: direction === "incoming" ? relatedPath : focus,
    target: direction === "incoming" ? focus : relatedPath,
    resolver: `symbol-${relation}`,
  }
}

function collectImportGroups(
  edges: GraphEdge[],
  groups: Map<string, MutableGroup>,
  typePaths: Set<string>,
  focus: string,
  symbol: GraphSymbol
): GraphEdge[] {
  const imports: GraphEdge[] = []
  for (const edge of edges) {
    const direction = fileEdgeDirection(edge, focus)
    if (!direction) continue
    const relatedPath = relatedFilePath(edge, direction)
    if (relatedPath === focus || typePaths.has(relatedPath)) continue
    appendGroup(groups, importGroup(focus, symbol, direction), relatedPath)
    imports.push(edge)
  }
  return imports
}

function fileEdgeDirection(
  edge: GraphEdge,
  focus: string
): TypeNeighborhoodDirection | null {
  if (edge.target === focus) return "incoming"
  if (edge.source === focus) return "outgoing"
  return null
}

function relatedFilePath(
  edge: GraphEdge,
  direction: TypeNeighborhoodDirection
): string {
  return direction === "incoming" ? edge.source : edge.target
}

function distinctGroups(groups: Map<string, MutableGroup>): {
  groups: Array<MutableGroup & { totalMembers: number }>
  claimed: Set<string>
} {
  const ordered = [...groups.values()]
    .map((group) => ({ ...group, paths: [...new Set(group.paths)].sort() }))
    .sort(compareGroups)
  const claimed = new Set<string>()
  const distinct = ordered
    .map((group) => ({
      ...group,
      paths: group.paths.filter((path) => claimPath(claimed, path)),
    }))
    .filter((group) => group.paths.length > 0)
    .map((group) => ({ ...group, totalMembers: group.paths.length }))
  return { groups: distinct, claimed }
}

function claimPath(claimed: Set<string>, path: string): boolean {
  if (claimed.has(path)) return false
  claimed.add(path)
  return true
}

function selectFocusSymbol(
  symbols: GraphSymbol[],
  edges: GraphSymbolEdge[],
  focus: string,
  preferredId?: string
): GraphSymbol | null {
  const incident = new Map<string, number>()
  for (const edge of edges) {
    incident.set(edge.source, (incident.get(edge.source) ?? 0) + 1)
    incident.set(edge.target, (incident.get(edge.target) ?? 0) + 1)
  }
  const candidates = symbols
    .filter(
      (symbol) => symbol.path === focus && (incident.get(symbol.id) ?? 0) > 0
    )
    .sort(
      (left, right) =>
        Number(right.id === preferredId) - Number(left.id === preferredId) ||
        (incident.get(right.id) ?? 0) - (incident.get(left.id) ?? 0) ||
        right.fan_in - left.fan_in ||
        right.fan_out - left.fan_out ||
        left.line - right.line
    )
  return candidates[0] ?? null
}

function relationshipGroup(
  focus: string,
  symbol: GraphSymbol,
  relation: TypeNeighborhoodRelation,
  direction: TypeNeighborhoodDirection
): Omit<MutableGroup, "paths"> {
  const incoming = direction === "incoming"
  const label = incoming
    ? relationLabel(relation)
    : relation === "implements"
      ? "Implemented contracts"
      : relation === "embeds"
        ? "Embedded contracts"
        : "Base types"
  const action =
    relation === "implements"
      ? "implement"
      : relation === "embeds"
        ? "embed"
        : "extend"
  return {
    id: `relationship:${focus}:type:${direction}:${relation}`,
    label,
    name: symbol.name,
    family: "type",
    direction,
    relation,
    description: incoming
      ? `Declared repository types that explicitly ${action} ${symbol.qualified_name}.`
      : `${symbol.qualified_name} explicitly ${action}s these repository types.`,
  }
}

function importGroup(
  focus: string,
  symbol: GraphSymbol,
  direction: TypeNeighborhoodDirection
): Omit<MutableGroup, "paths"> {
  const incoming = direction === "incoming"
  return {
    id: `relationship:${focus}:import:${direction}`,
    label: incoming ? "Import dependents" : "Import dependencies",
    name: symbol.name,
    family: "import",
    direction,
    relation: "imports",
    description: incoming
      ? `Files that directly import ${focus} without a resolved type relationship to ${symbol.name}.`
      : `Files directly imported by ${focus}.`,
  }
}

function appendGroup(
  groups: Map<string, MutableGroup>,
  group: Omit<MutableGroup, "paths">,
  path: string
) {
  const existing = groups.get(group.id)
  if (existing) existing.paths.push(path)
  else groups.set(group.id, { ...group, paths: [path] })
}

function retainGroups(
  groups: Array<MutableGroup & { totalMembers: number }>,
  limit: number
): TypeNeighborhoodGroup[] {
  const maximumGroups = Math.min(
    groups.length,
    Math.floor(Math.max(0, limit - 1) / 2)
  )
  const candidates = groups.slice(0, maximumGroups).map((group) => ({
    ...group,
    paths:
      group.family === "import"
        ? group.paths.slice(0, IMPORT_CONTEXT_LIMIT)
        : group.paths,
  }))
  const retained: TypeNeighborhoodGroup[] = candidates.map((group) => ({
    ...group,
    paths: [],
  }))
  let memberBudget = Math.max(0, limit - 1 - retained.length)

  for (let index = 0; index < retained.length && memberBudget > 0; index += 1) {
    retained[index].paths.push(candidates[index].paths[0])
    memberBudget -= 1
  }
  for (let index = 0; index < retained.length && memberBudget > 0; index += 1) {
    for (const path of candidates[index].paths.slice(1)) {
      if (memberBudget === 0) break
      retained[index].paths.push(path)
      memberBudget -= 1
    }
  }
  return retained.filter((group) => group.paths.length > 0)
}

function compareGroups(left: MutableGroup, right: MutableGroup): number {
  return (
    Number(left.family === "import") - Number(right.family === "import") ||
    Number(left.direction === "outgoing") -
      Number(right.direction === "outgoing") ||
    (RELATION_ORDER.get(left.relation) ?? 9) -
      (RELATION_ORDER.get(right.relation) ?? 9) ||
    left.id.localeCompare(right.id)
  )
}

function dedupeEdges(edges: GraphEdge[]): GraphEdge[] {
  const unique = new Map<string, GraphEdge>()
  for (const edge of edges) {
    unique.set(`${edge.source}\0${edge.target}\0${edge.resolver}`, edge)
  }
  return [...unique.values()].sort(
    (left, right) =>
      left.source.localeCompare(right.source) ||
      left.target.localeCompare(right.target) ||
      left.resolver.localeCompare(right.resolver)
  )
}

function relationLabel(relation: TypeNeighborhoodRelation): string {
  if (relation === "implements") return "Implements"
  if (relation === "embeds") return "Embeds"
  return "Extends"
}

function isTypeRelation(
  relation: string
): relation is TypeNeighborhoodRelation {
  return (
    relation === "extends" || relation === "implements" || relation === "embeds"
  )
}
