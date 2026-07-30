import {
  inspectGraphFile,
  projectGraph,
  structuralFileEdges,
  type GraphDirection,
  type GraphFileInspection,
} from "@/lib/graph-data"
import {
  projectTypeNeighborhood,
  type TypeNeighborhoodDirection,
  type TypeNeighborhoodFamily,
} from "@/lib/graph-type-neighborhood"
import {
  classifyFile,
  type ExplorerFileCategory,
} from "@/lib/graph-file-classification"
import {
  fileId,
  joinPath,
  normalizeScope,
  pathInScope,
  pathParent,
  relativeToScope,
  scopeId,
} from "@/lib/graph-explorer-paths"
import { compareGraphEdges, fileMatchRank } from "@/lib/graph-explorer-ranking"
import type {
  DependencyGraph,
  FileReport,
  FindingRecord,
  GraphEdge,
  GraphFile,
  RiskEntry,
  ScanReport,
} from "@/lib/types"

export const EXPLORER_ROOT = ""
export const EXPLORER_NODE_LIMIT = 100

export type ExplorerPresentation = "architecture" | "neighborhood" | "type"
export type ExplorerNeighborhoodPresentation = "auto" | "full" | "type"

export {
  classifyFile,
  type ExplorerFileCategory,
} from "@/lib/graph-file-classification"

export interface ExplorerLanguageStat {
  name: string
  files: number
}

export interface ExplorerGroup {
  id: string
  path: string
  name: string
  kind: "architecture" | "path" | "relationship"
  label: string
  members: ExplorerEntity[]
  totalMembers?: number
  languages: ExplorerLanguageStat[]
  relationship?: {
    family: TypeNeighborhoodFamily
    direction: TypeNeighborhoodDirection
    relation: ExplorerConnection["relation"]
    description: string
    focusPath: string
  }
}

export interface ExplorerScopeSummary {
  kind: "scope"
  id: string
  path: string
  name: string
  scopeKind: "project" | "package" | "area" | "directory"
  external: boolean
  files: number
  graphFiles: number
  tokens: number
  sloc: number
  findings: number
  riskFiles: number
  maxCyclomatic: number
  minMaintainability: number | null
  fanIn: number
  fanOut: number
  languages: ExplorerLanguageStat[]
}

export interface ExplorerFileSummary {
  kind: "file"
  id: string
  path: string
  name: string
  parentPath: string
  category: ExplorerFileCategory
  external: boolean
  report: FileReport
  graphFile: GraphFile | null
}

export type ExplorerEntity = ExplorerScopeSummary | ExplorerFileSummary

export interface ExplorerResolverUsage {
  resolver: string
  connections: number
}

export interface ExplorerConnection {
  id: string
  source: string
  target: string
  count: number
  relation:
    | "imports"
    | "includes"
    | "declares-module"
    | "imports-package"
    | "extends"
    | "implements"
    | "embeds"
    | "mixed"
  resolvers: ExplorerResolverUsage[]
  fileEdges: GraphEdge[]
}

export interface ExplorerBreadcrumb {
  path: string
  label: string
}

export interface ExplorerView {
  presentation: ExplorerPresentation
  focusPath: string | null
  scope: ExplorerScopeSummary
  breadcrumbs: ExplorerBreadcrumb[]
  entities: ExplorerEntity[]
  groups?: ExplorerGroup[]
  connections: ExplorerConnection[]
  totalEntities: number
  truncated: boolean
}

export interface ExplorerScopeInspection extends ExplorerScopeSummary {
  allFiles: ExplorerFileSummary[]
  directFiles: ExplorerFileSummary[]
  configFiles: string[]
}

export interface ExplorerFileInspection {
  file: ExplorerFileSummary
  graph: GraphFileInspection | null
  findings: FindingRecord[]
  risk: RiskEntry | null
}

export interface RepositoryGraphExplorer {
  view(scopePath: string): ExplorerView
  neighborhood(
    path: string,
    direction: GraphDirection,
    depth: number,
    presentation?: ExplorerNeighborhoodPresentation
  ): ExplorerView
  inspectScope(scopePath: string): ExplorerScopeInspection
  inspectFile(path: string): ExplorerFileInspection | null
  search(query: string, limit?: number): ExplorerFileSummary[]
  parentScope(path: string): string
}

interface ModelIndexes {
  graph: DependencyGraph
  structuralEdges: GraphEdge[]
  report: ScanReport
  reportFiles: FileReport[]
  reportsByPath: Map<string, FileReport>
  graphFilesByPath: Map<string, GraphFile>
  findingsByPath: Map<string, FindingRecord[]>
  risksByPath: Map<string, RiskEntry>
  anchorDirectories: Set<string>
}

const ARCHITECTURE_CONTAINERS = new Set([
  "apps",
  "crates",
  "modules",
  "packages",
  "plugins",
  "services",
])

const PACKAGE_MANIFESTS = new Set([
  "cargo.toml",
  "composer.json",
  "deno.json",
  "deno.jsonc",
  "go.mod",
  "package.json",
  "pyproject.toml",
])

export function buildRepositoryGraphExplorer(
  graph: DependencyGraph,
  report: ScanReport
): RepositoryGraphExplorer {
  const reportFiles = [...report.files].sort((left, right) =>
    left.path.localeCompare(right.path)
  )
  const indexes: ModelIndexes = {
    graph,
    structuralEdges: structuralFileEdges(graph),
    report,
    reportFiles,
    reportsByPath: new Map(reportFiles.map((file) => [file.path, file])),
    graphFilesByPath: new Map(graph.files.map((file) => [file.path, file])),
    findingsByPath: groupByPath(
      report.finding_catalog.findings,
      (finding) => finding.primary_location.path
    ),
    risksByPath: new Map(
      report.summary.top_risks.map((risk) => [risk.path, risk])
    ),
    anchorDirectories: anchorDirectories(graph, reportFiles),
  }

  return {
    view: (scopePath) => buildView(indexes, normalizeScope(scopePath)),
    neighborhood: (path, direction, depth, presentation = "auto") =>
      buildNeighborhood(indexes, path, direction, depth, presentation),
    inspectScope: (scopePath) =>
      inspectScope(indexes, normalizeScope(scopePath)),
    inspectFile: (path) => inspectFile(indexes, path),
    search: (query, limit = 10) => searchFiles(indexes, query, limit),
    parentScope: (path) => pathParent(path),
  }
}

function buildView(indexes: ModelIndexes, scopePath: string): ExplorerView {
  const scope = summarizeScope(indexes, scopePath, false)
  const entities = new Map<string, ExplorerEntity>()
  const groups = buildArchitectureGroups(indexes, scopePath, entities)
  if (groups.length === 0) addScopeContents(indexes, scopePath, entities)

  const mappedGraphFiles = new Map<string, string>()
  for (const file of indexes.graph.files) {
    const entity = entityForPath(file.path, scopePath, [...entities.values()])
    if (entity) mappedGraphFiles.set(file.path, entity.id)
  }

  const connectionEdges: Array<[GraphEdge, string, string]> = []
  for (const edge of indexes.structuralEdges) {
    const sourceInside = pathInScope(edge.source, scopePath)
    const targetInside = pathInScope(edge.target, scopePath)
    if (!sourceInside && !targetInside) continue

    const source =
      mappedGraphFiles.get(edge.source) ??
      ensureBoundaryEntity(indexes, entities, scopePath, edge.source)
    const target =
      mappedGraphFiles.get(edge.target) ??
      ensureBoundaryEntity(indexes, entities, scopePath, edge.target)
    if (source === target) continue
    connectionEdges.push([edge, source, target])
  }

  const connections = aggregateConnections(connectionEdges)
  const ranked = rankEntities([...entities.values()], connections)
  const totalEntities = ranked.length
  const totalRenderedNodes = ranked.length + groups.length
  const retained = ranked.slice(
    0,
    Math.max(1, EXPLORER_NODE_LIMIT - groups.length)
  )
  const retainedIds = new Set(retained.map((entity) => entity.id))
  const retainedGroups = groups
    .map((group) => ({
      ...group,
      members: group.members.filter((member) => retainedIds.has(member.id)),
    }))
    .filter((group) => group.members.length > 0)

  return {
    presentation: "architecture",
    focusPath: null,
    scope,
    breadcrumbs: breadcrumbs(scopePath),
    entities: retained.sort(compareEntities),
    groups: retainedGroups,
    connections: connections.filter(
      (connection) =>
        retainedIds.has(connection.source) && retainedIds.has(connection.target)
    ),
    totalEntities,
    truncated: totalRenderedNodes > EXPLORER_NODE_LIMIT,
  }
}

function buildNeighborhood(
  indexes: ModelIndexes,
  focus: string,
  direction: GraphDirection,
  depth: number,
  presentation: ExplorerNeighborhoodPresentation
): ExplorerView {
  const architecture = buildView(indexes, pathParent(focus))
  if (presentation !== "full") {
    const typeProjection = projectTypeNeighborhood(
      indexes.graph,
      focus,
      EXPLORER_NODE_LIMIT
    )
    if (typeProjection) {
      const entities = typeProjection.files.flatMap((path) =>
        indexes.reportsByPath.has(path)
          ? [summarizeFile(indexes, path, false)]
          : []
      )
      const entitiesByPath = new Map(
        entities.map((entity) => [entity.path, entity])
      )
      const groups = typeProjection.groups.flatMap((group) => {
        const members = group.paths.flatMap((path) => {
          const entity = entitiesByPath.get(path)
          return entity ? [entity] : []
        })
        if (members.length === 0) return []
        return [
          {
            id: group.id,
            path: group.id,
            name: group.name,
            kind: "relationship" as const,
            label: group.label,
            members,
            totalMembers: group.totalMembers,
            languages: languageStats(members),
            relationship: {
              family: group.family,
              direction: group.direction,
              relation: group.relation,
              description: group.description,
              focusPath: focus,
            },
          },
        ]
      })
      return {
        ...architecture,
        presentation: "type",
        focusPath: focus,
        entities,
        groups,
        connections: connectExplorerFiles(entities, typeProjection.edges),
        totalEntities: typeProjection.totalFiles,
        truncated: typeProjection.truncated,
      }
    }
  }

  const projection = projectGraph(indexes.graph, focus, direction, depth)
  const entities = projection.files.flatMap((file) =>
    indexes.reportsByPath.has(file.path)
      ? [summarizeFile(indexes, file.path, false)]
      : []
  )
  return {
    ...architecture,
    presentation: "neighborhood",
    focusPath: focus,
    entities,
    groups: undefined,
    connections: connectExplorerFiles(entities, projection.edges),
    totalEntities: projection.totalFiles,
    truncated: projection.truncated,
  }
}

function buildArchitectureGroups(
  indexes: ModelIndexes,
  scopePath: string,
  entities: Map<string, ExplorerEntity>
): ExplorerGroup[] {
  if (scopePath !== EXPLORER_ROOT) return []

  const groups: ExplorerGroup[] = []
  for (const root of rootChildScopes(indexes)) {
    const members = new Map<string, ExplorerEntity>()
    addScopeContents(indexes, root, members, false)
    if (
      members.size === 0 ||
      (members.size === 1 && !indexes.anchorDirectories.has(root))
    ) {
      const summary = summarizeScope(indexes, root, false)
      entities.set(summary.id, summary)
      continue
    }
    const scope = summarizeScope(indexes, root, false)
    for (const member of members.values()) entities.set(member.id, member)
    groups.push({
      id: `architecture-group:${root}`,
      path: root,
      name: scope.name,
      kind: "architecture",
      label:
        scope.scopeKind === "package"
          ? "Package"
          : scope.scopeKind === "area"
            ? "Project area"
            : "Directory",
      members: [...members.values()].sort(compareEntities),
      languages: scope.languages,
    })
  }
  for (const file of directFiles(indexes, scopePath)) {
    const summary = summarizeFile(indexes, file.path, false)
    entities.set(summary.id, summary)
  }
  return groups
}

function addScopeContents(
  indexes: ModelIndexes,
  scopePath: string,
  entities: Map<string, ExplorerEntity>,
  collapseChildren = true
) {
  const childScopes = collapseChildren
    ? visibleChildScopes(indexes, scopePath)
    : immediateChildScopes(indexes, scopePath).sort()
  for (const child of childScopes) {
    const summary = summarizeScope(indexes, child, false)
    entities.set(summary.id, summary)
  }
  for (const file of directFiles(indexes, scopePath)) {
    const summary = summarizeFile(indexes, file.path, false)
    entities.set(summary.id, summary)
  }
}

function inspectScope(
  indexes: ModelIndexes,
  scopePath: string
): ExplorerScopeInspection {
  const allFiles = filesInScope(indexes, scopePath).map((file) =>
    summarizeFile(indexes, file.path, false)
  )
  return {
    ...summarizeScope(indexes, scopePath, false),
    allFiles,
    directFiles: directFiles(indexes, scopePath).map((file) =>
      summarizeFile(indexes, file.path, false)
    ),
    configFiles:
      indexes.graph.config_files?.filter((path) =>
        pathInScope(path, scopePath)
      ) ?? [],
  }
}

function inspectFile(
  indexes: ModelIndexes,
  path: string
): ExplorerFileInspection | null {
  if (!indexes.reportsByPath.has(path)) return null
  return {
    file: summarizeFile(indexes, path, false),
    graph: inspectGraphFile(indexes.graph, path),
    findings: indexes.findingsByPath.get(path) ?? [],
    risk: indexes.risksByPath.get(path) ?? null,
  }
}

function searchFiles(
  indexes: ModelIndexes,
  query: string,
  limit: number
): ExplorerFileSummary[] {
  const normalized = query.trim().toLowerCase()
  if (!normalized) return []
  return indexes.reportFiles
    .filter((file) => file.path.toLowerCase().includes(normalized))
    .sort(
      (left, right) =>
        fileMatchRank(left.path, normalized) -
          fileMatchRank(right.path, normalized) ||
        graphConnectivity(indexes, right.path) -
          graphConnectivity(indexes, left.path) ||
        left.path.localeCompare(right.path)
    )
    .slice(0, Math.max(1, limit))
    .map((file) => summarizeFile(indexes, file.path, false))
}

function visibleChildScopes(
  indexes: ModelIndexes,
  scopePath: string
): string[] {
  if (scopePath === EXPLORER_ROOT) {
    return rootChildScopes(indexes)
  }

  const children = immediateChildScopes(indexes, scopePath)
  return children.map((child) => collapseScope(indexes, child)).sort()
}

function rootChildScopes(indexes: ModelIndexes): string[] {
  const roots = new Set<string>()
  for (const file of indexes.reportFiles) {
    const directory = pathParent(file.path)
    if (!directory) continue
    const parts = directory.split("/")
    const root =
      ARCHITECTURE_CONTAINERS.has(parts[0]) && parts.length > 1
        ? `${parts[0]}/${parts[1]}`
        : parts[0]
    roots.add(collapseScope(indexes, root))
  }
  return [...roots].sort()
}

function immediateChildScopes(
  indexes: ModelIndexes,
  scopePath: string
): string[] {
  const children = new Set<string>()
  for (const file of filesInScope(indexes, scopePath)) {
    const relative = relativeToScope(file.path, scopePath)
    const parts = relative.split("/")
    if (parts.length > 1) children.add(joinPath(scopePath, parts[0]))
  }
  return [...children]
}

function collapseScope(indexes: ModelIndexes, initial: string): string {
  let scope = initial
  while (
    !indexes.anchorDirectories.has(scope) &&
    directFiles(indexes, scope).length === 0
  ) {
    const children = immediateChildScopes(indexes, scope)
    if (children.length !== 1) break
    scope = children[0]
  }
  return scope
}

function ensureBoundaryEntity(
  indexes: ModelIndexes,
  entities: Map<string, ExplorerEntity>,
  currentScope: string,
  path: string
): string {
  const boundaryScope = boundaryScopeForPath(currentScope, path)
  if (!boundaryScope) {
    const file = summarizeFile(indexes, path, true)
    entities.set(file.id, file)
    return file.id
  }
  const id = scopeId(boundaryScope, true)
  if (!entities.has(id)) {
    const scope = summarizeScope(indexes, boundaryScope, true)
    entities.set(id, scope)
  }
  return id
}

function boundaryScopeForPath(currentScope: string, path: string): string {
  const directory = pathParent(path)
  if (!directory) return EXPLORER_ROOT
  const current = currentScope ? currentScope.split("/") : []
  const outside = directory.split("/")
  let shared = 0
  while (
    shared < current.length &&
    shared < outside.length &&
    current[shared] === outside[shared]
  ) {
    shared += 1
  }
  if (shared === outside.length && shared < current.length) return EXPLORER_ROOT
  const depth = Math.min(outside.length, shared + 1)
  return outside.slice(0, depth).join("/")
}

function entityForPath(
  path: string,
  currentScope: string,
  entities: ExplorerEntity[]
): ExplorerEntity | null {
  if (!pathInScope(path, currentScope)) return null
  return (
    entities.find((entity) =>
      entity.kind === "file"
        ? entity.path === path
        : pathInScope(path, entity.path)
    ) ?? null
  )
}

function aggregateConnections(
  edges: Array<[GraphEdge, string, string]>
): ExplorerConnection[] {
  const grouped = new Map<
    string,
    { source: string; target: string; edges: GraphEdge[] }
  >()
  for (const [edge, source, target] of edges) {
    const id = `${source}→${target}`
    const group = grouped.get(id)
    if (group) group.edges.push(edge)
    else grouped.set(id, { source, target, edges: [edge] })
  }

  return [...grouped.entries()]
    .map(([id, group]) => {
      const resolverCounts = new Map<string, number>()
      for (const edge of group.edges) {
        resolverCounts.set(
          edge.resolver,
          (resolverCounts.get(edge.resolver) ?? 0) + 1
        )
      }
      const relations = new Set(
        group.edges.map((edge) => edgeRelation(edge.resolver))
      )
      return {
        id,
        source: group.source,
        target: group.target,
        count: group.edges.length,
        relation: relations.size === 1 ? [...relations][0] : "mixed",
        resolvers: [...resolverCounts.entries()]
          .map(([resolver, connections]) => ({ resolver, connections }))
          .sort(
            (left, right) =>
              right.connections - left.connections ||
              left.resolver.localeCompare(right.resolver)
          ),
        fileEdges: [...group.edges].sort(compareGraphEdges),
      } satisfies ExplorerConnection
    })
    .sort(
      (left, right) =>
        left.source.localeCompare(right.source) ||
        left.target.localeCompare(right.target)
    )
}

function summarizeScope(
  indexes: ModelIndexes,
  scopePath: string,
  external: boolean
): ExplorerScopeSummary {
  const files = filesInScope(indexes, scopePath)
  const graphPaths = new Set(
    indexes.graph.files
      .filter((file) => pathInScope(file.path, scopePath))
      .map((file) => file.path)
  )
  const complexity = files.flatMap((file) =>
    file.complexity ? [file.complexity] : []
  )
  const languages = new Map<string, number>()
  for (const file of files) {
    languages.set(file.language, (languages.get(file.language) ?? 0) + 1)
  }
  let fanIn = 0
  let fanOut = 0
  for (const edge of indexes.structuralEdges) {
    const sourceInside = graphPaths.has(edge.source)
    const targetInside = graphPaths.has(edge.target)
    if (sourceInside && !targetInside) fanOut += 1
    if (!sourceInside && targetInside) fanIn += 1
  }

  return {
    kind: "scope",
    id: scopeId(scopePath, external),
    path: scopePath,
    name: scopeName(scopePath),
    scopeKind: scopeKind(indexes, scopePath),
    external,
    files: files.length,
    graphFiles: graphPaths.size,
    tokens: files.reduce((total, file) => total + file.tokens, 0),
    sloc: files.reduce((total, file) => total + file.sloc, 0),
    findings: indexes.report.finding_catalog.findings.filter((finding) =>
      pathInScope(finding.primary_location.path, scopePath)
    ).length,
    riskFiles: indexes.report.summary.top_risks.filter((risk) =>
      pathInScope(risk.path, scopePath)
    ).length,
    maxCyclomatic: complexity.reduce(
      (maximum, value) => Math.max(maximum, value.cyclomatic),
      0
    ),
    minMaintainability:
      complexity.length > 0
        ? Math.min(...complexity.map((value) => value.maintainability_index))
        : null,
    fanIn,
    fanOut,
    languages: [...languages.entries()]
      .map(([name, count]) => ({ name, files: count }))
      .sort(
        (left, right) =>
          right.files - left.files || left.name.localeCompare(right.name)
      ),
  }
}

function scopeName(scopePath: string): string {
  if (!scopePath) return "Project"
  return scopePath.split("/").at(-1) ?? scopePath
}

function summarizeFile(
  indexes: ModelIndexes,
  path: string,
  external: boolean
): ExplorerFileSummary {
  const report = indexes.reportsByPath.get(path)
  if (!report) {
    throw new Error(`graph file ${path} was not retained in the scan report`)
  }
  return {
    kind: "file",
    id: fileId(path, external),
    path,
    name: path.split("/").at(-1) ?? path,
    parentPath: pathParent(path),
    category: classifyFile(report),
    external,
    report,
    graphFile: indexes.graphFilesByPath.get(path) ?? null,
  }
}

function directFiles(indexes: ModelIndexes, scopePath: string): FileReport[] {
  return indexes.reportFiles.filter(
    (file) => pathParent(file.path) === scopePath
  )
}

function filesInScope(indexes: ModelIndexes, scopePath: string): FileReport[] {
  return indexes.reportFiles.filter((file) => pathInScope(file.path, scopePath))
}

function scopeKind(
  indexes: ModelIndexes,
  scopePath: string
): ExplorerScopeSummary["scopeKind"] {
  if (!scopePath) return "project"
  if (indexes.anchorDirectories.has(scopePath)) return "package"
  if (
    !scopePath.includes("/") ||
    ARCHITECTURE_CONTAINERS.has(scopePath.split("/")[0])
  ) {
    return "area"
  }
  return "directory"
}

export function groupExplorerFiles(
  entities: ExplorerEntity[]
): ExplorerGroup[] {
  const grouped = new Map<string, ExplorerFileSummary[]>()
  for (const entity of entities) {
    if (entity.kind !== "file") continue
    const files = grouped.get(entity.parentPath)
    if (files) files.push(entity)
    else grouped.set(entity.parentPath, [entity])
  }

  return [...grouped.entries()]
    .map(([path, files]) => {
      const sortedFiles = [...files].sort((left, right) =>
        left.path.localeCompare(right.path)
      )
      return {
        id: `group:${path || "."}`,
        path,
        name: path ? (path.split("/").at(-1) ?? path) : "Project root",
        kind: "path",
        label: fileGroupLabel(sortedFiles),
        members: sortedFiles,
        languages: languageStats(sortedFiles),
      } satisfies ExplorerGroup
    })
    .sort((left, right) => left.path.localeCompare(right.path))
}

function languageStats(files: ExplorerFileSummary[]): ExplorerLanguageStat[] {
  const languages = new Map<string, number>()
  for (const file of files) {
    languages.set(
      file.report.language,
      (languages.get(file.report.language) ?? 0) + 1
    )
  }
  return [...languages.entries()]
    .map(([name, count]) => ({ name, files: count }))
    .sort(
      (left, right) =>
        right.files - left.files || left.name.localeCompare(right.name)
    )
}

export function connectExplorerFiles(
  files: ExplorerFileSummary[],
  edges: GraphEdge[]
): ExplorerConnection[] {
  const ids = new Map(files.map((file) => [file.path, file.id]))
  return aggregateConnections(
    edges.flatMap((edge) => {
      const source = ids.get(edge.source)
      const target = ids.get(edge.target)
      if (!source || !target) return []
      const connection: [GraphEdge, string, string] = [edge, source, target]
      return [connection]
    })
  )
}

function fileGroupLabel(files: ExplorerFileSummary[]): ExplorerGroup["label"] {
  const families = new Set(
    files.map((file) => languageFamily(file.report.language))
  )
  if (families.size !== 1) return "Mixed-language scope"
  switch ([...families][0]) {
    case "rust":
      return "Module scope"
    case "go":
    case "python":
      return "Package scope"
    case "php":
      return "Namespace path"
    case "javascript":
      return "Module directory"
    default:
      return "Directory"
  }
}

function languageFamily(language: string): string {
  const normalized = language.toLowerCase()
  if (normalized === "rust") return "rust"
  if (normalized === "go") return "go"
  if (normalized.includes("python")) return "python"
  if (normalized === "php") return "php"
  if (
    normalized.includes("javascript") ||
    normalized.includes("typescript") ||
    normalized === "jsx" ||
    normalized === "tsx"
  )
    return "javascript"
  return normalized
}

function edgeRelation(resolver: string): ExplorerConnection["relation"] {
  if (resolver === "symbol-extends") return "extends"
  if (resolver === "symbol-implements") return "implements"
  if (resolver === "symbol-embeds") return "embeds"
  if (resolver === "php-include") return "includes"
  if (resolver === "rust-mod" || resolver === "rust-path")
    return "declares-module"
  if (resolver.startsWith("go-")) return "imports-package"
  return "imports"
}

function rankEntities(
  entities: ExplorerEntity[],
  connections: ExplorerConnection[]
): ExplorerEntity[] {
  const connectionWeight = new Map<string, number>()
  for (const connection of connections) {
    connectionWeight.set(
      connection.source,
      (connectionWeight.get(connection.source) ?? 0) + connection.count
    )
    connectionWeight.set(
      connection.target,
      (connectionWeight.get(connection.target) ?? 0) + connection.count
    )
  }
  return [...entities].sort((left, right) => {
    const leftWeight = connectionWeight.get(left.id) ?? 0
    const rightWeight = connectionWeight.get(right.id) ?? 0
    const leftFiles = left.kind === "scope" ? left.files : 1
    const rightFiles = right.kind === "scope" ? right.files : 1
    return (
      Number(left.external) - Number(right.external) ||
      rightWeight - leftWeight ||
      rightFiles - leftFiles ||
      compareEntities(left, right)
    )
  })
}

function compareEntities(left: ExplorerEntity, right: ExplorerEntity): number {
  return (
    Number(left.external) - Number(right.external) ||
    Number(left.kind === "file") - Number(right.kind === "file") ||
    left.path.localeCompare(right.path)
  )
}

function breadcrumbs(scopePath: string): ExplorerBreadcrumb[] {
  const result: ExplorerBreadcrumb[] = [
    { path: EXPLORER_ROOT, label: "Project" },
  ]
  if (!scopePath) return result
  const parts = scopePath.split("/")
  for (let index = 0; index < parts.length; index += 1) {
    result.push({
      path: parts.slice(0, index + 1).join("/"),
      label: parts[index],
    })
  }
  return result
}

function anchorDirectories(
  graph: DependencyGraph,
  files: FileReport[]
): Set<string> {
  const directories = new Set<string>()
  for (const path of [
    ...files.map((file) => file.path),
    ...(graph.config_files ?? []),
  ]) {
    const name = path.split("/").at(-1)?.toLowerCase() ?? ""
    if (PACKAGE_MANIFESTS.has(name)) directories.add(pathParent(path))
  }
  return directories
}

function groupByPath<T>(
  items: T[],
  path: (item: T) => string
): Map<string, T[]> {
  const grouped = new Map<string, T[]>()
  for (const item of items) {
    const key = path(item)
    const values = grouped.get(key)
    if (values) values.push(item)
    else grouped.set(key, [item])
  }
  return grouped
}

function graphConnectivity(indexes: ModelIndexes, path: string): number {
  const file = indexes.graphFilesByPath.get(path)
  return file ? file.fan_in + file.fan_out : 0
}
