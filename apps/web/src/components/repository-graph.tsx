import { memo, useCallback, useEffect, useMemo, useState } from "react"
import { useLocation, useNavigate } from "react-router"
import {
  AlertTriangle,
  ArrowDownToLine,
  ArrowUpFromLine,
  Boxes,
  Check,
  ChevronRight,
  Copy,
  FileCode2,
  Gauge,
  GitBranch,
  Layers3,
  Maximize2,
  Minimize2,
  Network,
  RotateCcw,
  Search,
} from "lucide-react"
import {
  Background,
  BackgroundVariant,
  BaseEdge,
  Controls,
  EdgeLabelRenderer,
  Handle,
  MarkerType,
  MiniMap,
  Panel,
  Position,
  ReactFlow,
  getBezierPath,
  useStore,
  type Edge,
  type EdgeProps,
  type Node,
  type NodeMouseHandler,
  type NodeProps,
} from "@xyflow/react"
import "@xyflow/react/dist/style.css"

import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card"
import { Skeleton } from "@/components/ui/skeleton"
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs"
import { useRepositoryGraph } from "@/hooks/use-repository-graph"
import {
  buildRepositoryGraphExplorer,
  type ExplorerConnection,
  type ExplorerEntity,
  type ExplorerGroup,
  type ExplorerFileInspection,
  type ExplorerFileSummary,
  type ExplorerLanguageStat,
  type ExplorerNeighborhoodPresentation,
  type ExplorerScopeInspection,
  type ExplorerView,
  type RepositoryGraphExplorer,
} from "@/lib/graph-explorer-model"
import {
  graphFileName,
  graphProminence,
  type GraphDirection,
  type GraphProminence,
} from "@/lib/graph-data"
import {
  isDenseExplorerView,
  layoutExplorerView,
  type ExplorerLayout,
} from "@/lib/graph-explorer-layout"
import {
  GRAPH_ROOT_ROUTE,
  graphRoutePath,
  parseGraphRoute,
  type GraphRoute,
  type GraphRouteDepth,
} from "@/lib/graph-routes"
import { formatCompact, formatNumber, formatScore } from "@/lib/format"
import type { DependencyGraph, GraphEdge, ScanReport } from "@/lib/types"
import { cn } from "@/lib/utils"

interface RepositoryGraphProps {
  revision: number
  report: ScanReport
}

interface GraphRouteNavigationOptions {
  replace?: boolean
}

type NavigateGraphRoute = (
  route: GraphRoute,
  options?: GraphRouteNavigationOptions,
) => void

interface ExplorerNodeData extends Record<string, unknown> {
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

type ExplorerNode = Node<ExplorerNodeData, "explorerEntity">

interface ExplorerGroupNodeData extends Record<string, unknown> {
  group: ExplorerGroup
  related: boolean
  dimmed: boolean
}

type ExplorerGroupFlowNode = Node<ExplorerGroupNodeData, "explorerGroup">
type ExplorerFlowNode = ExplorerNode | ExplorerGroupFlowNode

interface ExplorerEdgeData extends Record<string, unknown> {
  connection: ExplorerConnection
  showLabel: boolean
}

type ExplorerEdge = Edge<ExplorerEdgeData, "explorerConnection">

type GraphSelection =
  | { kind: "scope"; path: string }
  | { kind: "file"; path: string }
  | { kind: "group"; id: string }
  | { kind: "connection"; id: string }
  | null

type ExplorerMode = "architecture" | "neighborhood"

// Below this zoom the canvas is a map, not a document: node cards swap their
// dense metric rows for one large glanceable name so the overview stays legible.
const GLANCE_MAX_ZOOM = 0.5

const glanceZoomSelector = (state: { transform: [number, number, number] }): boolean =>
  state.transform[2] < GLANCE_MAX_ZOOM

const ExplorerEntityNode = memo(function ExplorerEntityNode({
  data,
  selected,
}: NodeProps<ExplorerNode>) {
  const { entity } = data
  const glance = useStore(glanceZoomSelector)
  const handleColor = entity.kind === "file"
    ? languageColor(entity.report.language)
    : scopeColor(entity)
  const targetPosition = data.vertical ? Position.Top : Position.Left
  const sourcePosition = data.vertical ? Position.Bottom : Position.Right

  return (
    <div
      style={{ width: data.width, height: data.height }}
      className={cn(
        "relative h-full cursor-pointer rounded-xl border bg-card/95 text-card-foreground shadow-sm backdrop-blur transition-[border-color,box-shadow,opacity,filter]",
        entity.kind === "scope"
          ? "px-4 py-3"
          : data.prominence.level === "hub"
            ? "px-4 py-3"
            : data.prominence.level === "notable"
              ? "px-3.5 py-3"
              : "px-3 py-2.5",
        entity.external && "border-dashed bg-card/75",
        entity.kind === "file" && data.prominence.level === "hub" && "border-primary/55 shadow-lg ring-1 ring-primary/10",
        entity.kind === "file" && data.prominence.level === "notable" && "border-foreground/30 shadow-md",
        data.typeFocused && "border-primary shadow-xl ring-2 ring-primary/25",
        data.focused && "border-foreground shadow-md ring-2 ring-foreground/10",
        data.related && !selected && "border-foreground/45 shadow-sm",
        selected && "border-ring shadow-md ring-2 ring-ring/25",
        !selected && "hover:border-foreground/60",
        data.dimmed && "opacity-25 saturate-50",
      )}
    >
      <span
        className="absolute inset-y-2 left-0 w-1 rounded-r-full"
        style={{ background: handleColor }}
      />
      <Handle
        type="target"
        position={targetPosition}
        className="!size-2 !border-background"
        style={{ background: handleColor }}
      />
      {entity.kind === "scope"
        ? glance
          ? <ScopeNodeGlance scope={entity} />
          : <ScopeNodeContent scope={entity} />
        : glance
          ? <FileNodeGlance file={entity} />
          : <FileNodeContent file={entity} prominence={data.prominence} />}
      <Handle
        type="source"
        position={sourcePosition}
        className="!size-2 !border-background"
        style={{ background: handleColor }}
      />
    </div>
  )
})

function ScopeNodeContent({ scope }: { scope: Extract<ExplorerEntity, { kind: "scope" }> }) {
  return (
    <div className="min-w-0 pl-1">
      <div className="flex items-center justify-between gap-3 text-[10px] font-semibold uppercase tracking-wide">
        <span style={{ color: scopeColor(scope) }}>
          {scope.external ? "Boundary" : scopeKindLabel(scope.scopeKind)}
        </span>
        <span className="text-muted-foreground">
          {scope.findings > 0 ? `${scope.findings} signals` : `${scope.graphFiles} connected`}
        </span>
      </div>
      <div className="mt-2 truncate text-sm font-semibold" title={scope.path || "Project"}>
        {scope.name}
      </div>
      <div className="mt-1 truncate font-mono text-[10px] text-muted-foreground">
        {scope.path || "."}
      </div>
      <div className="mt-3 flex items-center gap-3 text-[10px] text-muted-foreground tabular-nums">
        <span>{scope.files} files</span>
        <span>{scope.fanIn} in</span>
        <span>{scope.fanOut} out</span>
      </div>
      <div className="mt-2 flex min-w-0 gap-1 overflow-hidden">
        {scope.languages.slice(0, 3).map((language) => (
          <span key={language.name} className="truncate rounded-full bg-muted px-1.5 py-0.5 text-[9px] text-muted-foreground">
            {language.name} {language.files}
          </span>
        ))}
        {scope.languages.length > 3 ? (
          <span className="rounded-full bg-muted px-1.5 py-0.5 text-[9px] text-muted-foreground">
            +{scope.languages.length - 3}
          </span>
        ) : null}
      </div>
    </div>
  )
}

function FileNodeContent({ file, prominence }: { file: ExplorerFileSummary; prominence: GraphProminence }) {
  const graph = file.graphFile
  const metric = file.report.complexity ? `C${file.report.complexity.cyclomatic}` : file.report.language
  return (
    <div className="min-w-0 pl-1">
      <div className="flex items-center justify-between gap-3 text-[10px] font-semibold uppercase tracking-wide">
        <span style={{ color: languageColor(file.report.language) }}>{categoryLabel(file.category)}</span>
        <span
          className={cn(
            "truncate text-muted-foreground",
            prominence.level === "hub" && "text-primary",
            prominence.level === "notable" && "text-foreground/75",
          )}
          title={prominence.reason}
        >
          {prominence.level === "standard"
            ? metric
            : prominence.basis === "symbol"
              ? `${prominence.label} · ${prominence.reach}`
              : prominence.label}
        </span>
      </div>
      <div className="mt-1.5 truncate font-mono text-xs font-semibold" title={file.path}>
        {file.name}
      </div>
      <div className="mt-0.5 truncate font-mono text-[10px] text-muted-foreground">
        {file.parentPath || "."}
      </div>
      <div className="mt-2 flex gap-3 text-[10px] text-muted-foreground tabular-nums">
        <span>{formatCompact(file.report.tokens)} tok</span>
        <span>{graph ? `${graph.fan_in} in · ${graph.fan_out} out` : "metrics only"}</span>
      </div>
    </div>
  )
}

function ScopeNodeGlance({ scope }: { scope: Extract<ExplorerEntity, { kind: "scope" }> }) {
  return (
    <div className="flex h-full min-w-0 flex-col items-center justify-center gap-1 px-1 text-center">
      <span className="w-full truncate text-2xl font-bold leading-tight" title={scope.path || "Project"}>
        {scope.name}
      </span>
      <span className="w-full truncate text-sm text-muted-foreground tabular-nums">
        {scope.files} files · {scope.fanIn} in · {scope.fanOut} out
      </span>
    </div>
  )
}

function FileNodeGlance({ file }: { file: ExplorerFileSummary }) {
  const graph = file.graphFile
  return (
    <div className="flex h-full min-w-0 flex-col items-center justify-center gap-1 px-1 text-center">
      <span className="w-full truncate font-mono text-xl font-bold leading-tight" title={file.path}>
        {file.name}
      </span>
      <span className="w-full truncate text-sm text-muted-foreground tabular-nums">
        {graph ? `${graph.fan_in} in · ${graph.fan_out} out` : file.report.language}
      </span>
    </div>
  )
}

const ExplorerGroupNode = memo(function ExplorerGroupNode({
  data,
  selected,
}: NodeProps<ExplorerGroupFlowNode>) {
  const { group } = data
  const glance = useStore(glanceZoomSelector)
  const visibleMembers = group.members.length
  const totalMembers = group.totalMembers ?? visibleMembers
  const memberCount = visibleMembers < totalMembers
    ? `${visibleMembers} of ${totalMembers}`
    : String(visibleMembers)
  return (
    <div
      className={cn(
        "h-full w-full cursor-pointer rounded-2xl border transition-[border-color,background-color,opacity]",
        group.kind === "architecture"
          ? "border-border/80 bg-card/45 shadow-md"
          : group.kind === "relationship" && group.relationship?.family === "type"
            ? "border-primary/55 bg-primary/5 shadow-md"
            : "border-dashed bg-muted/15 shadow-inner",
        selected && "border-ring bg-ring/5 ring-2 ring-ring/20",
        data.related && !selected && "border-foreground/40 bg-muted/25",
        data.dimmed && "opacity-20",
      )}
    >
      <div className={cn(
        "flex h-12 items-center justify-between gap-3 border-b px-4 text-muted-foreground",
        glance ? "text-lg" : "text-[10px] uppercase tracking-wide",
        group.kind === "architecture"
          ? "bg-muted/30"
          : group.kind === "relationship" && group.relationship?.family === "type"
            ? "border-primary/25 bg-primary/10 text-primary"
            : "border-dashed",
      )}>
        <span className="min-w-0 truncate font-semibold" title={group.path || "."}>
          {group.label} · {group.name}
        </span>
        <span className="shrink-0 tabular-nums">
          {memberCount} {group.kind === "architecture" ? "items" : "files"}
        </span>
      </div>
    </div>
  )
})

const nodeTypes = { explorerEntity: ExplorerEntityNode, explorerGroup: ExplorerGroupNode }

const ExplorerConnectionEdge = memo(function ExplorerConnectionEdge({
  id,
  sourceX,
  sourceY,
  targetX,
  targetY,
  sourcePosition,
  targetPosition,
  markerEnd,
  style,
  data,
}: EdgeProps<ExplorerEdge>) {
  const [path, labelX, labelY] = getBezierPath({
    sourceX,
    sourceY,
    sourcePosition,
    targetX,
    targetY,
    targetPosition,
    curvature: 0.32,
  })

  return (
    <>
      <BaseEdge id={id} path={path} markerEnd={markerEnd} interactionWidth={28} style={style} />
      {data?.showLabel ? (
        <EdgeLabelRenderer>
          <div
            className="repository-graph-edge-label nodrag nopan pointer-events-none absolute whitespace-nowrap rounded-full border bg-popover/95 px-2 py-0.5 text-[9px] font-medium text-popover-foreground shadow-sm"
            style={{ transform: `translate(-50%, -50%) translate(${labelX}px, ${labelY}px)` }}
          >
            {relationLabel(data.connection.relation)}
            {data.connection.count > 1 ? ` · ${data.connection.count}` : ""}
          </div>
        </EdgeLabelRenderer>
      ) : null}
    </>
  )
})

const edgeTypes = { explorerConnection: ExplorerConnectionEdge }

export function RepositoryGraph({ revision, report }: RepositoryGraphProps) {
  const location = useLocation()
  const navigate = useNavigate()
  const route = useMemo(
    () => parseGraphRoute(location.pathname, location.search),
    [location.pathname, location.search],
  )
  const navigateGraph = useCallback<NavigateGraphRoute>((next, options) => {
    navigate(graphRoutePath(next), { replace: options?.replace })
  }, [navigate])
  const request = useRepositoryGraph(revision)

  useEffect(() => {
    const canonical = graphRoutePath(route ?? GRAPH_ROOT_ROUTE)
    if (`${location.pathname}${location.search}` !== canonical) {
      navigate(canonical, { replace: true })
    }
  }, [location.pathname, location.search, navigate, route])

  if (request.loading) return <GraphLoading />
  if (request.error) {
    return (
      <Card>
        <CardHeader>
          <CardTitle className="flex items-center gap-2"><AlertTriangle className="size-4" /> Graph unavailable</CardTitle>
          <CardDescription>{request.error}</CardDescription>
        </CardHeader>
        <CardContent>
          <Button variant="outline" onClick={request.retry}><RotateCcw /> Try again</Button>
        </CardContent>
      </Card>
    )
  }
  if (!request.graph || request.graph.files.length === 0) {
    return (
      <Card>
        <CardHeader>
          <CardTitle>No first-class graph files</CardTitle>
          <CardDescription>
            Repository topology supports Rust, Python, JavaScript, TypeScript/TSX, Go, and PHP.
          </CardDescription>
        </CardHeader>
      </Card>
    )
  }
  return (
    <GraphExplorer
      graph={request.graph}
      report={report}
      route={route ?? GRAPH_ROOT_ROUTE}
      onRouteChange={navigateGraph}
    />
  )
}

function GraphLoading() {
  return (
    <Card aria-label="Building repository graph">
      <CardHeader>
        <CardTitle className="flex items-center gap-2"><Network className="size-4 animate-pulse" /> Building repository graph</CardTitle>
        <CardDescription>This analysis runs only when the Graph tab is opened.</CardDescription>
      </CardHeader>
      <CardContent className="grid gap-4 xl:grid-cols-[minmax(0,1fr)_28rem]">
        <Skeleton className="h-[52rem] w-full" />
        <Skeleton className="h-[52rem] w-full" />
      </CardContent>
    </Card>
  )
}

function GraphExplorer({
  graph,
  report,
  route,
  onRouteChange,
}: {
  graph: DependencyGraph
  report: ScanReport
  route: GraphRoute
  onRouteChange: NavigateGraphRoute
}) {
  const explorer = useMemo(() => buildRepositoryGraphExplorer(graph, report), [graph, report])
  const routedFile = useMemo(
    () => route.kind === "file" ? explorer.inspectFile(route.focus) : null,
    [explorer, route],
  )
  const validFileRoute = route.kind === "file" && Boolean(routedFile?.graph)
  const validScopeRoute = useMemo(
    () => route.kind !== "architecture"
      || !route.scopePath
      || report.files.some((file) => file.path.startsWith(`${route.scopePath}/`)),
    [report.files, route],
  )
  const mode: ExplorerMode = validFileRoute ? "neighborhood" : "architecture"
  const focus = validFileRoute && route.kind === "file" ? route.focus : null
  const scopePath = route.kind === "architecture"
    ? (validScopeRoute ? route.scopePath : "")
    : routedFile
      ? explorer.parentScope(route.focus)
      : ""
  const direction = route.kind === "file" ? route.direction : "both"
  const depth = route.kind === "file" ? route.depth : 2
  const neighborhoodPresentation = route.kind === "file" ? route.presentation : "auto"
  const [selection, setSelection] = useState<GraphSelection>(() =>
    route.kind === "file" && routedFile ? { kind: "file", path: route.focus } : null
  )
  const [search, setSearch] = useState(route.kind === "file" ? route.focus : "")
  const [searchOpen, setSearchOpen] = useState(false)
  const [expanded, setExpanded] = useState(false)
  const [hoveredEntityId, setHoveredEntityId] = useState<string | null>(null)

  const architectureView = useMemo(() => explorer.view(scopePath), [explorer, scopePath])
  const display = useMemo(
    () => mode === "neighborhood" && focus
      ? explorer.neighborhood(focus, direction, depth, neighborhoodPresentation)
      : architectureView,
    [architectureView, depth, direction, explorer, focus, mode, neighborhoodPresentation],
  )
  const matches = useMemo(() => explorer.search(search), [explorer, search])
  const selectedConnection = selection?.kind === "connection"
    ? display.connections.find((connection) => connection.id === selection.id) ?? null
    : null
  const selectedEntityId = selection?.kind === "file" || selection?.kind === "scope"
    ? display.entities.find((entity) =>
        entity.kind === selection.kind && entity.path === selection.path
      )?.id ?? null
    : null
  const selectedScopePath = selection?.kind === "scope" ? selection.path : null
  const selectedGroupId = selection?.kind === "group" ? selection.id : null
  const placements = useMemo(
    () => layoutExplorerView(display, display.presentation),
    [display],
  )
  const layout = useMemo(
    () => decorateLayout(
      display,
      placements,
      focus,
      selectedEntityId,
      selectedScopePath,
      selectedGroupId,
      selectedConnection?.id ?? null,
      hoveredEntityId,
    ),
    [display, focus, hoveredEntityId, placements, selectedConnection?.id, selectedEntityId, selectedGroupId, selectedScopePath],
  )
  const legend = useMemo(
    () => buildCanvasLegend(display, placements.prominence),
    [display, placements],
  )
  const visibleGroups = layout.nodes.filter((node) => node.type === "explorerGroup").length
  const typeFocused = display.presentation === "type"
  const typeFocusAvailable = Boolean(
    focus && explorer.inspectFile(focus)?.graph?.symbolRelations.length,
  )
  const denseView = !typeFocused && isDenseExplorerView(display)
  const initialFitMinZoom = denseView
    ? 0.42
    : typeFocused
      ? 0.32
      : display.entities.length > 18
        ? 0.28
        : 0.16

  const openScope = useCallback((path: string) => {
    setSelection(null)
    setSearchOpen(false)
    onRouteChange({ kind: "architecture", scopePath: path })
  }, [onRouteChange])

  const locateFile = useCallback((path: string) => {
    setSelection({ kind: "file", path })
    setSearch(path)
    setSearchOpen(false)
    onRouteChange({ kind: "architecture", scopePath: explorer.parentScope(path) })
  }, [explorer, onRouteChange])

  const exploreFile = useCallback((
    path: string,
    nextDirection: GraphDirection = "both",
    presentation: ExplorerNeighborhoodPresentation = "auto",
  ) => {
    setSelection({ kind: "file", path })
    setSearch(path)
    setSearchOpen(false)
    onRouteChange({
      kind: "file",
      focus: path,
      direction: nextDirection,
      depth: 2,
      presentation,
    })
  }, [onRouteChange])

  const updateNeighborhoodRoute = useCallback((updates: {
    direction?: GraphDirection
    depth?: GraphRouteDepth
    presentation?: ExplorerNeighborhoodPresentation
  }) => {
    if (route.kind !== "file" || !validFileRoute) return
    onRouteChange({ ...route, ...updates })
  }, [onRouteChange, route, validFileRoute])

  useEffect(() => {
    if (mode === "neighborhood" && focus) {
      setSelection({ kind: "file", path: focus })
      setSearch(focus)
    } else {
      setSelection((current) =>
        current?.kind === "file" && explorer.parentScope(current.path) === scopePath
          ? current
          : null
      )
    }
    setSearchOpen(false)
  }, [explorer, focus, mode, scopePath])

  useEffect(() => {
    if (route.kind === "architecture") {
      if (!validScopeRoute) onRouteChange(GRAPH_ROOT_ROUTE, { replace: true })
      return
    }
    if (validFileRoute) return
    setSelection(routedFile ? { kind: "file", path: route.focus } : null)
    setSearch(routedFile ? route.focus : "")
    onRouteChange({
      kind: "architecture",
      scopePath: routedFile ? explorer.parentScope(route.focus) : "",
    }, { replace: true })
  }, [explorer, onRouteChange, route, routedFile, validFileRoute, validScopeRoute])

  const submitSearch = (event: React.FormEvent) => {
    event.preventDefault()
    const exact = report.files.find((file) => file.path === search.trim())
    const match = exact ? explorer.inspectFile(exact.path)?.file : matches[0]
    if (match) locateFile(match.path)
  }

  const onNodeClick = useCallback<NodeMouseHandler<ExplorerFlowNode>>((_, node) => {
    if (node.type === "explorerGroup") {
      setSelection(node.data.group.kind === "relationship"
        ? { kind: "group", id: node.data.group.id }
        : { kind: "scope", path: node.data.group.path })
      return
    }
    const entity = node.data.entity
    setSelection({ kind: entity.kind, path: entity.path })
  }, [])
  const onNodeDoubleClick = useCallback<NodeMouseHandler<ExplorerFlowNode>>((_, node) => {
    if (node.type === "explorerGroup") {
      if (node.data.group.kind === "relationship") {
        setSelection({ kind: "group", id: node.data.group.id })
      } else {
        openScope(node.data.group.path)
      }
      return
    }
    const entity = node.data.entity
    if (entity.kind === "scope") openScope(entity.path)
    else exploreFile(entity.path)
  }, [exploreFile, openScope])
  const onEdgeClick = useCallback((_: React.MouseEvent, edge: ExplorerEdge) => {
    if (edge.data?.connection) {
      setSelection({ kind: "connection", id: edge.data.connection.id })
    }
  }, [])
  const onNodeMouseEnter = useCallback<NodeMouseHandler<ExplorerFlowNode>>((_, node) => {
    setHoveredEntityId(node.type === "explorerEntity" ? node.id : null)
  }, [])
  const onNodeMouseLeave = useCallback(() => setHoveredEntityId(null), [])

  useEffect(() => {
    setHoveredEntityId(null)
  }, [display])

  useEffect(() => {
    const previousOverflow = document.body.style.overflow
    if (expanded) document.body.style.overflow = "hidden"
    const onEscape = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return
      if (expanded) {
        setExpanded(false)
      } else if (searchOpen) {
        setSearchOpen(false)
      } else if (selection) {
        setSelection(null)
      } else if (mode === "neighborhood") {
        openScope(focus ? explorer.parentScope(focus) : "")
      } else if (architectureView.breadcrumbs.length > 1) {
        openScope(architectureView.breadcrumbs.at(-2)?.path ?? "")
      }
    }
    window.addEventListener("keydown", onEscape)
    return () => {
      document.body.style.overflow = previousOverflow
      window.removeEventListener("keydown", onEscape)
    }
  }, [architectureView.breadcrumbs, expanded, explorer, focus, mode, openScope, searchOpen, selection])

  const hubFiles = useMemo(
    () => graph.files.filter((file) => graphProminence(file).level === "hub").length,
    [graph],
  )
  const topDepended = graph.top_depended[0] ?? null
  const largestCycle = graph.cycles.reduce((largest, cycle) => Math.max(largest, cycle.length), 0)

  return (
    <div className="space-y-4">
      <section className="grid grid-cols-2 gap-3 lg:grid-cols-3 xl:grid-cols-6">
        <GraphMetric label="First-class files" value={graph.nodes} detail={graph.languages.join(", ")} />
        <GraphMetric
          label="Relationships"
          value={graph.edges + (graph.symbol_edges?.length ?? 0)}
          detail={`${graph.edges} file · ${graph.symbol_edges?.length ?? 0} type`}
        />
        <GraphMetric
          label="Hub files"
          value={hubFiles}
          detail={topDepended
            ? `${graphFileName(topDepended.path)} leads · ${topDepended.fan_in} dependents`
            : "no wide-reach files"}
        />
        <GraphMetric
          label="Cycles"
          value={graph.cycles.length}
          detail={largestCycle > 0 ? `largest loops through ${largestCycle} files` : "no circular dependencies"}
          tone={graph.cycles.length > 0 ? "attention" : undefined}
        />
        <GraphMetric
          label="Orphans"
          value={graph.orphans.length}
          detail="never imported · not entrypoints or tests"
        />
        <GraphMetric
          label="Unresolved imports"
          value={graph.unresolved_imports}
          detail={`${graph.parse_errors ?? 0} parse · ${graph.config_errors ?? 0} config errors`}
        />
      </section>

      {expanded ? <div className="fixed inset-0 z-40 bg-background/80 backdrop-blur-sm" aria-hidden="true" /> : null}
      <Card
        data-expanded={expanded}
        className={cn(
          "repository-graph-workspace gap-4 overflow-hidden py-4",
          expanded && "fixed inset-2 z-50 gap-3 rounded-xl bg-card py-3 shadow-2xl sm:inset-4",
        )}
      >
        <CardHeader className="gap-4 px-4 sm:px-5">
          <div className="flex flex-wrap items-start justify-between gap-3">
            <div className="min-w-0">
              <GraphBreadcrumbs
                view={architectureView}
                mode={mode}
                focus={focus}
                onOpen={openScope}
              />
              <CardDescription className="mt-1">
                {typeFocused
                  ? "Type structure keeps the selected symbol central, groups explicit type relations, and quiets ordinary imports."
                  : "Hover a node to trace its links, click to inspect, double-click to open. Scroll pans; pinch or Ctrl+scroll zooms."}
              </CardDescription>
            </div>
            <div className="flex flex-wrap items-center justify-end gap-2">
              {graph.config_files?.slice(0, 3).map((path) => <Badge key={path} variant="outline">{path}</Badge>)}
              {(graph.config_files?.length ?? 0) > 3 ? <Badge variant="outline">+{graph.config_files!.length - 3} configs</Badge> : null}
              {(graph.config_errors ?? 0) > 0 ? <Badge variant="destructive">{graph.config_errors} config errors</Badge> : null}
              {(graph.parse_errors ?? 0) > 0 ? <Badge variant="secondary">{graph.parse_errors} parse errors</Badge> : null}
              <Button
                type="button"
                size="sm"
                variant="outline"
                aria-label={expanded ? "Restore graph workspace" : "Expand graph workspace"}
                aria-pressed={expanded}
                onClick={() => setExpanded((current) => !current)}
              >
                {expanded ? <Minimize2 /> : <Maximize2 />}
                {expanded ? "Restore" : "Expand"}
              </Button>
            </div>
          </div>

          <div className={cn(
            "grid gap-3",
            mode === "neighborhood" && typeFocusAvailable
              ? "lg:grid-cols-[minmax(16rem,1fr)_auto_auto_auto_auto]"
              : "lg:grid-cols-[minmax(16rem,1fr)_auto_auto_auto]",
          )}>
            <form className="relative flex min-w-0 gap-2" onSubmit={submitSearch}>
              <div className="relative min-w-0 flex-1">
                <Search className="pointer-events-none absolute left-3 top-1/2 size-4 -translate-y-1/2 text-muted-foreground" />
                <input
                  type="search"
                  role="combobox"
                  aria-label="Find a file in the repository graph"
                  aria-expanded={searchOpen && matches.length > 0}
                  aria-controls="repository-graph-search-results"
                  autoComplete="off"
                  value={search}
                  onChange={(event) => { setSearch(event.target.value); setSearchOpen(true) }}
                  onFocus={() => setSearchOpen(true)}
                  placeholder="Find any scanned file…"
                  className="h-9 w-full rounded-md border bg-background pl-9 pr-3 font-mono text-xs shadow-xs outline-none transition focus-visible:border-ring focus-visible:ring-3 focus-visible:ring-ring/50"
                />
                {searchOpen && matches.length > 0 ? (
                  <div
                    id="repository-graph-search-results"
                    role="listbox"
                    className="absolute inset-x-0 top-11 z-30 max-h-72 overflow-auto rounded-lg border bg-popover p-1 text-popover-foreground shadow-xl"
                  >
                    {matches.map((file) => (
                      <button
                        key={file.path}
                        type="button"
                        role="option"
                        aria-selected={selection?.kind === "file" && selection.path === file.path}
                        onMouseDown={(event) => event.preventDefault()}
                        onClick={() => locateFile(file.path)}
                        className="flex w-full items-center gap-3 rounded-md px-3 py-2 text-left hover:bg-accent focus-visible:bg-accent focus-visible:outline-none"
                      >
                        <span className="size-2 shrink-0 rounded-full" style={{ background: languageColor(file.report.language) }} />
                        <span className="min-w-0 flex-1">
                          <span className="block truncate font-mono text-xs font-medium">{file.path}</span>
                          <span className="block text-[10px] text-muted-foreground">
                            {categoryLabel(file.category)} · {file.report.language} · {formatCompact(file.report.tokens)} tokens
                          </span>
                        </span>
                      </button>
                    ))}
                  </div>
                ) : null}
              </div>
              <Button type="submit" variant="outline">Locate</Button>
            </form>

            <LabeledSelect
              label="Direction"
              value={direction}
              disabled={mode !== "neighborhood" || typeFocused}
              onChange={(value) => updateNeighborhoodRoute({ direction: value as GraphDirection })}
              options={[
                ["dependencies", "Dependencies"],
                ["dependents", "Blast radius"],
                ["both", "Both directions"],
              ]}
            />
            <LabeledSelect
              label="Depth"
              value={String(depth)}
              disabled={mode !== "neighborhood" || typeFocused}
              onChange={(value) => updateNeighborhoodRoute({ depth: Number(value) as GraphRouteDepth })}
              options={[["1", "1 hop"], ["2", "2 hops"], ["3", "3 hops"]]}
            />
            {mode === "neighborhood" && typeFocusAvailable ? (
              <Button
                type="button"
                variant={typeFocused ? "secondary" : "outline"}
                aria-label={typeFocused ? "Show full neighborhood" : "Show type structure"}
                onClick={() => updateNeighborhoodRoute({
                  presentation: typeFocused ? "full" : "type",
                })}
              >
                <GitBranch /> {typeFocused ? "Full neighborhood" : "Type structure"}
              </Button>
            ) : null}
            <Button
              type="button"
              variant="outline"
              disabled={mode === "architecture" && !scopePath}
              onClick={() => mode === "neighborhood"
                ? openScope(focus ? explorer.parentScope(focus) : scopePath)
                : openScope("")}
            >
              <Layers3 /> {mode === "neighborhood" ? "Architecture" : "Project"}
            </Button>
          </div>

          <div className="flex flex-wrap items-center gap-2 text-xs text-muted-foreground">
            <Badge variant="secondary">
              Showing {display.entities.length} of {display.totalEntities} {mode === "architecture" ? "items" : "files"}
            </Badge>
            <span>{display.connections.length} visible relationships</span>
            <span>{display.scope.graphFiles} of {display.scope.files} files participate in topology</span>
            {visibleGroups > 0 ? (
              <Badge variant="outline">
                {visibleGroups} {typeFocused ? "relationship" : mode === "architecture" ? "architecture" : "path-based"} {visibleGroups === 1 ? "group" : "groups"}
              </Badge>
            ) : null}
            {typeFocused ? <Badge variant="outline">Type structure · direct declared relationships</Badge> : null}
            {denseView ? <Badge variant="outline">Dense view · select a node to isolate direct links</Badge> : null}
            {display.truncated ? <Badge variant="outline">bounded view</Badge> : null}
          </div>
        </CardHeader>

        <CardContent
          className={cn(
            "grid min-h-0 gap-4 px-4 sm:px-5 xl:grid-cols-[minmax(0,1fr)_28rem]",
            expanded && "flex-1 overflow-auto lg:grid-cols-[minmax(0,1fr)_30rem] lg:overflow-hidden xl:grid-cols-[minmax(0,1fr)_30rem]",
          )}
        >
          <div
            className={cn(
              "repository-graph-flow h-[52rem] overflow-hidden rounded-xl border bg-muted/20",
              expanded && "h-[56dvh] min-h-80 lg:h-full",
            )}
            aria-label="Interactive repository dependency graph"
          >
            <ReactFlow<ExplorerFlowNode, ExplorerEdge>
              key={`${mode}:${display.presentation}:${scopePath}:${focus ?? "none"}:${direction}:${depth}:${expanded ? "expanded" : "inline"}`}
              nodes={layout.nodes}
              edges={layout.edges}
              nodeTypes={nodeTypes}
              edgeTypes={edgeTypes}
              onNodeClick={onNodeClick}
              onNodeDoubleClick={onNodeDoubleClick}
              onNodeMouseEnter={onNodeMouseEnter}
              onNodeMouseLeave={onNodeMouseLeave}
              onEdgeClick={onEdgeClick}
              onPaneClick={() => setSelection(null)}
              nodesDraggable={false}
              nodesConnectable={false}
              zoomOnDoubleClick={false}
              panOnScroll
              minZoom={0.08}
              maxZoom={1.5}
              fitView
              fitViewOptions={{ padding: 0.16, minZoom: initialFitMinZoom, maxZoom: 1 }}
              onlyRenderVisibleElements
              proOptions={{ hideAttribution: true }}
            >
              <Background variant={BackgroundVariant.Dots} gap={20} size={1} />
              <Controls
                showInteractive={false}
                fitViewOptions={{ padding: 0.12, minZoom: 0.08, maxZoom: 1 }}
              />
              <Panel position="top-right" className="pointer-events-none">
                <CanvasLegend legend={legend} typeFocused={typeFocused} />
              </Panel>
              {display.entities.length > 8 ? (
                <MiniMap
                  pannable
                  zoomable
                  ariaLabel="Repository graph overview"
                  style={{ width: expanded ? 260 : 224, height: expanded ? 176 : 152 }}
                  bgColor="#0b0f14"
                  maskColor="rgba(2, 6, 23, 0.66)"
                  maskStrokeColor="#94a3b8"
                  maskStrokeWidth={1.5}
                  nodeBorderRadius={4}
                  nodeStrokeColor="#020617"
                  nodeStrokeWidth={8}
                  nodeColor={(node) => {
                    const data = node.data as ExplorerNodeData | ExplorerGroupNodeData
                    return "group" in data ? "#334155" : miniMapEntityColor(data.entity)
                  }}
                />
              ) : null}
            </ReactFlow>
          </div>
          <GraphDetails
            explorer={explorer}
            view={display}
            selection={selection}
            connection={selectedConnection}
            expanded={expanded}
            onLocate={locateFile}
            onExplore={exploreFile}
          />
        </CardContent>
      </Card>
    </div>
  )
}

function GraphBreadcrumbs({
  view,
  mode,
  focus,
  onOpen,
}: {
  view: ExplorerView
  mode: ExplorerMode
  focus: string | null
  onOpen: (path: string) => void
}) {
  return (
    <nav aria-label="Graph location" className="flex flex-wrap items-center gap-1 text-sm font-semibold">
      {view.breadcrumbs.map((crumb, index) => (
        <span key={crumb.path || "."} className="flex items-center gap-1">
          {index > 0 ? <ChevronRight className="size-3.5 text-muted-foreground" /> : null}
          <button
            type="button"
            onClick={() => onOpen(crumb.path)}
            className="rounded px-1.5 py-1 hover:bg-accent focus-visible:bg-accent focus-visible:outline-none"
          >
            {crumb.label}
          </button>
        </span>
      ))}
      {mode === "neighborhood" && focus ? (
        <>
          <ChevronRight className="size-3.5 text-muted-foreground" />
          <span className="max-w-80 truncate rounded bg-muted px-2 py-1 font-mono text-xs">{focus}</span>
        </>
      ) : null}
      <span className="ml-2 text-xs font-normal text-muted-foreground">Esc to step back</span>
    </nav>
  )
}

function GraphMetric({
  label,
  value,
  detail,
  tone,
}: {
  label: string
  value: number
  detail: string
  tone?: "attention"
}) {
  return (
    <Card className="gap-1 py-4">
      <CardHeader className="gap-1 px-4 sm:px-5">
        <CardDescription>{label}</CardDescription>
        <CardTitle className={cn("text-2xl tabular-nums", tone === "attention" && value > 0 && "text-destructive")}>
          {value.toLocaleString()}
        </CardTitle>
        <p className="truncate text-xs text-muted-foreground" title={detail}>{detail || "—"}</p>
      </CardHeader>
    </Card>
  )
}

interface CanvasLegendData {
  languages: ExplorerLanguageStat[]
  moreLanguages: number
  hasTypeRelations: boolean
  hasImportContext: boolean
  hasExternal: boolean
  hasProminent: boolean
}

function buildCanvasLegend(
  view: ExplorerView,
  prominence: Map<string, GraphProminence>,
): CanvasLegendData {
  const languages = new Map<string, number>()
  for (const entity of view.entities) {
    if (entity.kind === "file") {
      languages.set(entity.report.language, (languages.get(entity.report.language) ?? 0) + 1)
    } else {
      for (const stat of entity.languages) {
        languages.set(stat.name, (languages.get(stat.name) ?? 0) + stat.files)
      }
    }
  }
  const ranked = [...languages.entries()]
    .map(([name, files]) => ({ name, files }))
    .sort((left, right) => right.files - left.files || left.name.localeCompare(right.name))
  const typeRelations = new Set(["extends", "implements", "embeds"])
  return {
    languages: ranked.slice(0, 5),
    moreLanguages: Math.max(0, ranked.length - 5),
    hasTypeRelations: view.connections.some((connection) => typeRelations.has(connection.relation)),
    hasImportContext: view.presentation === "type"
      && view.connections.some((connection) => !typeRelations.has(connection.relation)),
    hasExternal: view.entities.some((entity) => entity.external),
    hasProminent: [...prominence.values()].some((entry) => entry.level !== "standard"),
  }
}

function LegendEdgeSample({ dashed, color }: { dashed?: boolean; color: string }) {
  return (
    <svg width="18" height="6" aria-hidden="true" className="shrink-0">
      <line
        x1="0"
        y1="3"
        x2="18"
        y2="3"
        stroke={color}
        strokeWidth="1.5"
        strokeDasharray={dashed ? "4 3" : undefined}
      />
    </svg>
  )
}

function CanvasLegend({ legend, typeFocused }: { legend: CanvasLegendData; typeFocused: boolean }) {
  return (
    <div className="pointer-events-none w-44 rounded-lg border bg-card/90 p-2.5 text-[10px] leading-relaxed text-muted-foreground shadow-sm backdrop-blur">
      <p className="font-semibold uppercase tracking-wide">Legend</p>
      <div className="mt-1.5 space-y-0.5">
        {legend.languages.map((language) => (
          <p key={language.name} className="flex items-center gap-1.5">
            <span
              className="size-2 shrink-0 rounded-full"
              style={{ background: languageColor(language.name) }}
            />
            <span className="min-w-0 flex-1 truncate text-foreground/85">{language.name}</span>
            <span className="tabular-nums">{language.files}</span>
          </p>
        ))}
        {legend.moreLanguages > 0 ? <p>+{legend.moreLanguages} more languages</p> : null}
      </div>
      <div className="mt-2 space-y-1 border-t pt-2">
        <p className="flex items-center gap-1.5">
          <LegendEdgeSample color={typeFocused && legend.hasTypeRelations ? "var(--primary)" : "var(--muted-foreground)"} />
          <span className="min-w-0 flex-1">
            {typeFocused && legend.hasTypeRelations ? "declared type relation" : "arrow points at the dependency"}
          </span>
        </p>
        {legend.hasImportContext ? (
          <p className="flex items-center gap-1.5">
            <LegendEdgeSample dashed color="var(--muted-foreground)" />
            <span className="min-w-0 flex-1">direct import context</span>
          </p>
        ) : null}
        {legend.hasExternal ? (
          <p className="flex items-center gap-1.5">
            <span aria-hidden="true" className="h-3.5 w-[18px] shrink-0 rounded-sm border border-dashed border-muted-foreground/70" />
            <span className="min-w-0 flex-1">outside this scope</span>
          </p>
        ) : null}
        {legend.hasProminent ? (
          <p className="flex items-center gap-1.5">
            <span aria-hidden="true" className="flex w-[18px] shrink-0 items-end justify-between">
              <span className="size-1.5 rounded-[2px] border border-muted-foreground/70" />
              <span className="size-2.5 rounded-[2px] border border-muted-foreground/70" />
            </span>
            <span className="min-w-0 flex-1">larger card = wider proven reach</span>
          </p>
        ) : null}
      </div>
    </div>
  )
}

function LabeledSelect({
  label,
  value,
  disabled,
  options,
  onChange,
}: {
  label: string
  value: string
  disabled: boolean
  options: Array<[string, string]>
  onChange: (value: string) => void
}) {
  return (
    <label className="grid grid-cols-[auto_1fr] items-center gap-2 rounded-md border bg-background px-3 text-xs text-muted-foreground shadow-xs has-[:focus-visible]:border-ring has-[:focus-visible]:ring-3 has-[:focus-visible]:ring-ring/50 has-[:disabled]:opacity-50">
      {label}
      <select
        aria-label={`Graph ${label.toLowerCase()}`}
        value={value}
        disabled={disabled}
        onChange={(event) => onChange(event.target.value)}
        className="h-8 bg-transparent font-medium text-foreground outline-none"
      >
        {options.map(([option, text]) => <option key={option} value={option}>{text}</option>)}
      </select>
    </label>
  )
}

function GraphDetails({
  explorer,
  view,
  selection,
  connection,
  expanded,
  onLocate,
  onExplore,
}: {
  explorer: RepositoryGraphExplorer
  view: ExplorerView
  selection: GraphSelection
  connection: ExplorerConnection | null
  expanded: boolean
  onLocate: (path: string) => void
  onExplore: (
    path: string,
    direction?: GraphDirection,
    presentation?: ExplorerNeighborhoodPresentation,
  ) => void
}) {
  const className = cn(
    "h-[52rem] overflow-auto rounded-xl border bg-card",
    expanded && "h-auto min-h-80 lg:h-full",
  )
  if (selection?.kind === "connection" && connection) {
    return (
      <GraphConnectionDetails
        view={view}
        connection={connection}
        className={className}
        onLocate={onLocate}
      />
    )
  }
  if (selection?.kind === "group") {
    const group = view.groups?.find((candidate) => candidate.id === selection.id)
    if (group?.relationship) {
      return (
        <GraphRelationshipGroupDetails
          group={group}
          className={className}
          onExplore={onExplore}
        />
      )
    }
  }
  if (selection?.kind === "file") {
    const inspection = explorer.inspectFile(selection.path)
    if (inspection) {
      return (
        <GraphFileDetails
          inspection={inspection}
          className={className}
          onLocate={onLocate}
          onExplore={onExplore}
        />
      )
    }
  }
  if (selection?.kind === "scope") {
    return (
      <GraphScopeDetails
        inspection={explorer.inspectScope(selection.path)}
        className={className}
        onLocate={onLocate}
      />
    )
  }
  return (
    <GraphScopeDetails
      inspection={explorer.inspectScope(view.scope.path)}
      className={className}
      onLocate={onLocate}
    />
  )
}

function GraphRelationshipGroupDetails({
  group,
  className,
  onExplore,
}: {
  group: ExplorerGroup
  className: string
  onExplore: (
    path: string,
    direction?: GraphDirection,
    presentation?: ExplorerNeighborhoodPresentation,
  ) => void
}) {
  const relationship = group.relationship!
  const totalMembers = group.totalMembers ?? group.members.length
  const memberCount = group.members.length < totalMembers
    ? `${group.members.length} of ${totalMembers}`
    : String(group.members.length)
  return (
    <aside className={cn(className, "p-4")}>
      <div className="flex flex-wrap items-center gap-2">
        <Badge>{relationship.family === "type" ? "Explicit type relationship" : "Direct import context"}</Badge>
        <Badge variant="outline">{relationship.direction}</Badge>
        <Badge variant="secondary">{memberCount} files</Badge>
      </div>
      <h3 className="mt-3 text-base font-semibold">{group.label} {group.name}</h3>
      <p className="mt-2 text-xs leading-relaxed text-muted-foreground">
        {relationship.description}
      </p>
      <section className="mt-5 border-t pt-4">
        <h4 className="text-xs font-semibold uppercase tracking-wide text-muted-foreground">
          Members <span className="float-right tabular-nums">{memberCount}</span>
        </h4>
        <div className="mt-2 space-y-1.5">
          {group.members.map((member) => (
            <button
              key={member.id}
              type="button"
              onClick={() => member.kind === "file" && onExplore(member.path, "both", "auto")}
              className="flex w-full items-center gap-2 rounded-lg border bg-background px-3 py-2 text-left hover:bg-accent focus-visible:bg-accent focus-visible:outline-none"
            >
              <span
                className="size-2 shrink-0 rounded-full"
                style={{ background: member.kind === "file" ? languageColor(member.report.language) : scopeColor(member) }}
              />
              <span className="min-w-0 flex-1">
                <span className="block truncate font-mono text-[11px] font-medium">{member.path}</span>
                <span className="mt-0.5 block text-[10px] text-muted-foreground">
                  {member.kind === "file" ? `${member.report.language} · ${formatCompact(member.report.tokens)} tokens` : `${member.files} files`}
                </span>
              </span>
              <ChevronRight className="size-3.5 text-muted-foreground" />
            </button>
          ))}
        </div>
      </section>
    </aside>
  )
}

function GraphScopeDetails({
  inspection,
  className,
  onLocate,
}: {
  inspection: ExplorerScopeInspection
  className: string
  onLocate: (path: string) => void
}) {
  const scanned = inspection.allFiles
  const testFiles = scanned.filter((file) => file.category === "test").length
  const markerTotals = new Map<string, number>()
  for (const file of scanned) {
    for (const [marker, count] of Object.entries(file.report.markers ?? {})) {
      markerTotals.set(marker, (markerTotals.get(marker) ?? 0) + count)
    }
  }
  const markers = [...markerTotals.values()].reduce((total, count) => total + count, 0)
  const topMarker = [...markerTotals.entries()]
    .sort((left, right) => right[1] - left[1] || left[0].localeCompare(right[0]))
    .at(0)
  const churnCommits = scanned.reduce((total, file) => total + (file.report.churn?.commits ?? 0), 0)
  const filesWithHistory = scanned.filter((file) => (file.report.churn?.commits ?? 0) > 0).length
  const loc = scanned.reduce((total, file) => total + file.report.loc, 0)
  const commentLines = scanned.reduce((total, file) => total + file.report.comment_lines, 0)
  const commentPct = loc > 0 ? Math.round((commentLines / loc) * 100) : 0

  return (
    <aside className={cn(className, "p-4")}>
      <Tabs key={inspection.path || "."} defaultValue="info">
        <TabsList className="grid w-full grid-cols-2">
          <TabsTrigger value="info">Info</TabsTrigger>
          <TabsTrigger value="files">Files <span className="tabular-nums">{inspection.files}</span></TabsTrigger>
        </TabsList>
        <TabsContent value="info" className="pt-3">
          <div className="flex flex-wrap items-center gap-2">
            <Badge>{scopeKindLabel(inspection.scopeKind)}</Badge>
            {inspection.riskFiles > 0 ? <Badge variant="destructive">{inspection.riskFiles} risk files</Badge> : null}
            {inspection.findings > 0 ? <Badge variant="secondary">{inspection.findings} findings</Badge> : null}
          </div>
          <h3 className="mt-3 break-all text-base font-semibold">{inspection.name}</h3>
          <p className="mt-1 break-all font-mono text-xs text-muted-foreground">{inspection.path || "."}</p>

          <section className="mt-5">
            <h4 className="flex items-center gap-2 text-xs font-semibold uppercase tracking-wide text-muted-foreground">
              <Gauge className="size-3.5" /> Scope facts
            </h4>
            <div className="mt-2 grid grid-cols-2 gap-2">
              <DetailMetric label="Files" value={formatNumber(inspection.files)} detail={`${inspection.graphFiles} in topology`} />
              <DetailMetric label="Tokens" value={formatCompact(inspection.tokens)} detail={`${formatNumber(inspection.sloc)} SLOC`} />
              <DetailMetric label="Lines" value={formatCompact(loc)} detail={`${commentPct}% comments`} />
              <DetailMetric label="Test files" value={formatNumber(testFiles)} detail={`${formatNumber(scanned.length - testFiles)} non-test files`} />
              <DetailMetric label="Fan in / out" value={`${inspection.fanIn} / ${inspection.fanOut}`} detail="cross-scope edges" />
              <DetailMetric label="Max cyclomatic" value={formatNumber(inspection.maxCyclomatic)} detail="highest file total" />
              <DetailMetric
                label="Min maintainability"
                value={inspection.minMaintainability?.toFixed(1) ?? "—"}
                detail="lowest file index"
              />
              <DetailMetric
                label="Churn"
                value={formatNumber(churnCommits)}
                detail={filesWithHistory > 0 ? `commits · ${formatNumber(filesWithHistory)} files with history` : "no git history"}
              />
              <DetailMetric
                label="Markers"
                value={formatNumber(markers)}
                detail={topMarker ? `mostly ${topMarker[0]} ×${topMarker[1]}` : "no TODO-style markers"}
              />
              <DetailMetric label="Signals" value={formatNumber(inspection.findings)} detail={`${inspection.riskFiles} ranked risks`} />
            </div>
          </section>

          <section className="mt-5 border-t pt-4">
            <h4 className="flex items-center gap-2 text-xs font-semibold uppercase tracking-wide text-muted-foreground">
              <Boxes className="size-3.5" /> Languages
            </h4>
            <div className="mt-2 flex flex-wrap gap-1.5">
              {inspection.languages.map((language) => (
                <Badge key={language.name} variant="outline">
                  <span className="mr-1.5 size-1.5 rounded-full" style={{ background: languageColor(language.name) }} />
                  {language.name} · {language.files}
                </Badge>
              ))}
            </div>
          </section>

          <section className="mt-5 border-t pt-4">
            <h4 className="text-xs font-semibold uppercase tracking-wide text-muted-foreground">Topology coverage</h4>
            <p className="mt-2 rounded-lg bg-muted/60 px-3 py-2 text-xs leading-relaxed text-muted-foreground">
              {inspection.graphFiles} of {inspection.files} scanned files are first-class topology nodes. Other recognized files remain visible in the Files tab and scope totals without invented relationships.
            </p>
            {inspection.configFiles.length > 0 ? (
              <div className="mt-2 flex flex-wrap gap-1.5">
                {inspection.configFiles.map((path) => <Badge key={path} variant="secondary">{path}</Badge>)}
              </div>
            ) : null}
          </section>
        </TabsContent>
        <TabsContent value="files" className="pt-3">
          <div className="space-y-1.5">
            {inspection.allFiles.slice(0, 150).map((file) => (
              <button
                key={file.path}
                type="button"
                onClick={() => onLocate(file.path)}
                className="flex w-full items-center gap-2 rounded-lg border bg-background px-3 py-2 text-left hover:bg-accent focus-visible:bg-accent focus-visible:outline-none"
              >
                <span className="size-2 shrink-0 rounded-full" style={{ background: languageColor(file.report.language) }} />
                <span className="min-w-0 flex-1">
                  <span className="block truncate font-mono text-[11px]">{file.path}</span>
                  <span className="mt-0.5 block text-[10px] text-muted-foreground">
                    {categoryLabel(file.category)} · {file.report.language} · {formatCompact(file.report.tokens)} tokens
                  </span>
                </span>
              </button>
            ))}
            {inspection.allFiles.length > 150 ? (
              <p className="px-2 py-2 text-[10px] text-muted-foreground">
                Showing 150 of {inspection.allFiles.length} files. Use search to locate another file.
              </p>
            ) : null}
          </div>
        </TabsContent>
      </Tabs>
    </aside>
  )
}

function GraphFileDetails({
  inspection,
  className,
  onLocate,
  onExplore,
}: {
  inspection: ExplorerFileInspection
  className: string
  onLocate: (path: string) => void
  onExplore: (
    path: string,
    direction?: GraphDirection,
    presentation?: ExplorerNeighborhoodPresentation,
  ) => void
}) {
  const { file, graph, findings, risk } = inspection
  const report = file.report
  const markers = Object.values(report.markers ?? {}).reduce((total, count) => total + count, 0)
  const functions = [...(report.complexity?.functions ?? [])].sort(
    (left, right) => right.cyclomatic - left.cyclomatic || left.line - right.line,
  )

  return (
    <aside className={cn(className, "p-4")}>
      <Tabs key={file.path} defaultValue="info">
        <TabsList className="grid w-full grid-cols-2">
          <TabsTrigger value="info">Info</TabsTrigger>
          <TabsTrigger value="relations">Relations</TabsTrigger>
        </TabsList>
        <TabsContent value="info" className="pt-3">
          <div className="flex items-start justify-between gap-3">
            <div className="flex flex-wrap items-center gap-2">
              <Badge>{categoryLabel(file.category)}</Badge>
              <Badge variant="secondary">{report.language}</Badge>
              {risk ? <Badge variant={risk.score >= 0.7 ? "destructive" : "outline"}>Risk {formatScore(risk.score)}</Badge> : null}
            </div>
            <CopyPathButton path={file.path} />
          </div>
          <h3 className="mt-3 break-all font-mono text-sm font-semibold leading-relaxed">{file.path}</h3>

          {graph ? (
            <>
              <div className="mt-4 grid grid-cols-2 gap-2">
                {graph.symbolRelations.length > 0 ? (
                  <Button className="col-span-2" size="sm" onClick={() => onExplore(file.path, "both", "type")}>
                    <GitBranch /> Type structure
                  </Button>
                ) : null}
                <Button size="sm" variant="outline" onClick={() => onExplore(file.path, "dependencies", "full")}>
                  <ArrowDownToLine /> Dependencies
                </Button>
                <Button size="sm" variant="outline" onClick={() => onExplore(file.path, "dependents", "full")}>
                  <ArrowUpFromLine /> Blast radius
                </Button>
                <Button className="col-span-2" size="sm" variant="secondary" onClick={() => onExplore(file.path, "both", "full")}>
                  <Layers3 /> Full neighborhood
                </Button>
              </div>
              <section className="mt-5 rounded-lg border bg-muted/30 p-3">
                <div className="flex items-center justify-between gap-2">
                  <h4 className="flex items-center gap-2 text-xs font-semibold uppercase tracking-wide text-muted-foreground">
                    <Network className="size-3.5" /> Structural reach
                  </h4>
                  <Badge variant={graph.prominence.level === "hub" ? "default" : "outline"}>
                    {graph.prominence.label}
                  </Badge>
                </div>
                <p className="mt-2 text-xs leading-relaxed">{graph.prominence.reason}</p>
                <p className="mt-1.5 text-[10px] leading-relaxed text-muted-foreground">
                  {graph.prominence.basis === "symbol"
                    ? "Node size uses explicit extends, implements, or embeds syntax resolved to an unambiguous repository symbol."
                    : "Node size uses resolved file dependencies; ambiguous type relationships are never inferred."}
                </p>
              </section>
            </>
          ) : null}

          <section className="mt-5 border-t pt-4">
            <h4 className="flex items-center gap-2 text-xs font-semibold uppercase tracking-wide text-muted-foreground">
              <Gauge className="size-3.5" /> Latest scan
            </h4>
            <div className="mt-2 grid grid-cols-2 gap-2">
              <DetailMetric label="Tokens" value={formatCompact(report.tokens)} detail={`${formatNumber(report.sloc)} SLOC`} />
              <DetailMetric
                label="Lines"
                value={formatCompact(report.loc)}
                detail={`${Math.round(report.comment_ratio * 100)}% comments`}
              />
              <DetailMetric label="Fan in / out" value={graph ? `${graph.file.fan_in} / ${graph.file.fan_out}` : "—"} detail={graph ? "direct links" : "not in topology"} />
              <DetailMetric
                label="Cyclomatic"
                value={report.complexity ? formatNumber(report.complexity.cyclomatic) : "—"}
                detail={report.complexity ? `${formatNumber(report.complexity.cognitive)} cognitive` : "not measured"}
              />
              <DetailMetric
                label="Maintainability"
                value={report.complexity ? report.complexity.maintainability_index.toFixed(1) : "—"}
                detail={report.approximate ? "approximate" : "index"}
              />
              <DetailMetric
                label="Max nesting"
                value={report.complexity ? formatNumber(report.complexity.max_nesting) : "—"}
                detail={functions.length > 0 ? `${formatNumber(functions.length)} callables` : "no callables"}
              />
              <DetailMetric
                label="Churn"
                value={report.churn ? formatNumber(report.churn.commits) : "—"}
                detail={report.churn ? `${formatNumber(report.churn.authors)} authors` : "not measured"}
              />
              <DetailMetric label="Signals" value={formatNumber(findings.length)} detail={`${formatNumber(markers)} markers`} />
            </div>
            {report.churn?.first_commit || report.churn?.last_commit ? (
              <p className="mt-2 rounded-lg bg-muted/60 px-3 py-2 text-[10px] text-muted-foreground">
                History {shortDate(report.churn.first_commit)} → {shortDate(report.churn.last_commit)}
              </p>
            ) : null}
          </section>

          {report.symbols ? (
            <section className="mt-5 border-t pt-4">
              <h4 className="flex items-center gap-2 text-xs font-semibold uppercase tracking-wide text-muted-foreground">
                <FileCode2 className="size-3.5" /> Symbols
              </h4>
              <div className="mt-2 grid grid-cols-3 gap-2">
                <MiniMetric label="Functions" value={report.symbols.functions} />
                <MiniMetric label="Types" value={report.symbols.types} />
                <MiniMetric label="Exports" value={report.symbols.exports} />
              </div>
            </section>
          ) : null}

          {functions.length > 0 ? (
            <section className="mt-5 border-t pt-4">
              <h4 className="text-xs font-semibold uppercase tracking-wide text-muted-foreground">
                Callables <span className="float-right tabular-nums">{functions.length}</span>
              </h4>
              <div className="mt-2 space-y-1.5">
                {functions.slice(0, 20).map((fn) => (
                  <div key={`${fn.symbol_key ?? fn.name}:${fn.line}`} className="rounded-lg border px-3 py-2">
                    <div className="flex items-center justify-between gap-2">
                      <span className="min-w-0 truncate font-mono text-[11px] font-medium" title={fn.name}>{fn.name}</span>
                      <Badge variant={fn.cyclomatic > 20 ? "destructive" : "outline"}>C{fn.cyclomatic}</Badge>
                    </div>
                    <p className="mt-1 text-[10px] text-muted-foreground">
                      line {fn.line}{fn.end_line ? `–${fn.end_line}` : ""} · cognitive {fn.cognitive} · nesting {fn.max_nesting}
                    </p>
                  </div>
                ))}
              </div>
            </section>
          ) : null}

          {(report.marker_occurrences?.length ?? 0) > 0 ? (
            <section className="mt-5 border-t pt-4">
              <h4 className="text-xs font-semibold uppercase tracking-wide text-muted-foreground">Markers</h4>
              <div className="mt-2 flex flex-wrap gap-1.5">
                {report.marker_occurrences!.slice(0, 30).map((marker) => (
                  <Badge key={`${marker.marker}:${marker.line}:${marker.occurrence}`} variant="outline">
                    {marker.marker} · line {marker.line}
                  </Badge>
                ))}
              </div>
            </section>
          ) : null}

          {findings.length > 0 ? <FileFindings findings={findings} /> : null}
        </TabsContent>
        <TabsContent value="relations" className="pt-3">
          {!graph ? (
            <div className="rounded-lg border border-dashed p-4 text-sm text-muted-foreground">
              This recognized file contributes scope metrics but is not a first-class dependency node.
            </div>
          ) : (
            <>
              <div className="flex flex-wrap gap-1.5">
                {graph.roles.map((role) => <Badge key={role} variant="outline">{role}</Badge>)}
              </div>
              {graph.resolverUsage.length > 0 ? (
                <section className="mt-5 border-t pt-4">
                  <h4 className="flex items-center gap-2 text-xs font-semibold uppercase tracking-wide text-muted-foreground">
                    <Network className="size-3.5" /> Resolver provenance
                  </h4>
                  <div className="mt-2 flex flex-wrap gap-1.5">
                    {graph.resolverUsage.map((usage) => (
                      <Badge key={usage.resolver} variant="secondary">
                        {resolverLabel(usage.resolver)} · {usage.connections}
                      </Badge>
                    ))}
                  </div>
                </section>
              ) : null}
              {graph.symbolRelations.length > 0 ? (
                <section className="mt-5 border-t pt-4">
                  <h4 className="flex items-center gap-2 text-xs font-semibold uppercase tracking-wide text-muted-foreground">
                    <GitBranch className="size-3.5" /> Explicit type relationships
                  </h4>
                  <div className="mt-2 space-y-1.5">
                    {graph.symbolRelations.slice(0, 30).map(({ direction, relation, symbol }) => (
                      <button
                        key={`${direction}:${relation}:${symbol.id}`}
                        type="button"
                        onClick={() => onLocate(symbol.path)}
                        className="flex w-full items-center gap-2 rounded-lg border px-3 py-2 text-left hover:bg-accent"
                      >
                        <span className="text-muted-foreground">{direction === "incoming" ? "←" : "→"}</span>
                        <Badge variant="outline">{relation}</Badge>
                        <span className="min-w-0 flex-1">
                          <span className="block truncate font-mono text-[11px] font-medium">{symbol.qualified_name}</span>
                          <span className="block truncate text-[10px] text-muted-foreground">{symbol.path}:{symbol.line}</span>
                        </span>
                      </button>
                    ))}
                  </div>
                </section>
              ) : null}
              <ConnectionList title="Dependencies" icon={ArrowDownToLine} edges={graph.outgoing} pathKey="target" onLocate={onLocate} />
              <ConnectionList title="Dependents" icon={ArrowUpFromLine} edges={graph.incoming} pathKey="source" onLocate={onLocate} />
              {graph.cycles.length > 0 ? (
                <section className="mt-5 border-t pt-4">
                  <h4 className="flex items-center gap-2 text-xs font-semibold uppercase tracking-wide text-muted-foreground">
                    <GitBranch className="size-3.5" /> Cycle membership
                  </h4>
                  <div className="mt-2 space-y-2">
                    {graph.cycles.map((cycle, index) => (
                      <div key={cycle.join("→")} className="rounded-lg border border-destructive/25 bg-destructive/5 p-2.5">
                        <p className="text-[10px] font-semibold uppercase tracking-wide text-destructive">Cycle {index + 1} · {cycle.length} files</p>
                        {cycle.map((path) => (
                          <button key={path} type="button" onClick={() => onLocate(path)} className="mt-1 block w-full truncate rounded px-1 py-1 text-left font-mono text-[10px] hover:bg-accent">
                            {path}
                          </button>
                        ))}
                      </div>
                    ))}
                  </div>
                </section>
              ) : null}
              {(report.imports?.length ?? 0) > 0 ? (
                <section className="mt-5 border-t pt-4">
                  <h4 className="text-xs font-semibold uppercase tracking-wide text-muted-foreground">Imported roots</h4>
                  <div className="mt-2 flex flex-wrap gap-1.5">
                    {report.imports!.map((dependency) => <Badge key={dependency} variant="outline">{dependency}</Badge>)}
                  </div>
                </section>
              ) : null}
            </>
          )}
        </TabsContent>
      </Tabs>
    </aside>
  )
}

function GraphConnectionDetails({
  view,
  connection,
  className,
  onLocate,
}: {
  view: ExplorerView
  connection: ExplorerConnection
  className: string
  onLocate: (path: string) => void
}) {
  const source = view.entities.find((entity) => entity.id === connection.source)
  const target = view.entities.find((entity) => entity.id === connection.target)
  return (
    <aside className={cn(className, "p-4")}>
      <div className="flex flex-wrap items-center gap-2">
        <Badge>{relationLabel(connection.relation)}</Badge>
        <Badge variant="outline">{connection.count} file connection{connection.count === 1 ? "" : "s"}</Badge>
      </div>
      <h3 className="mt-3 text-sm font-semibold">Connection details</h3>

      <div className="mt-4 grid gap-2">
        <EntityEndpoint label="Source" entity={source} />
        <ArrowDownToLine className="mx-auto size-4 text-muted-foreground" />
        <EntityEndpoint label="Target" entity={target} />
      </div>

      <section className="mt-5 border-t pt-4">
        <h4 className="text-xs font-semibold uppercase tracking-wide text-muted-foreground">Resolver provenance</h4>
        <div className="mt-2 space-y-2">
          {connection.resolvers.map((usage) => (
            <div key={usage.resolver} className="rounded-lg bg-muted/60 px-3 py-2">
              <div className="flex items-center justify-between gap-2 text-xs font-medium">
                <span>{resolverLabel(usage.resolver)}</span>
                <span className="tabular-nums text-muted-foreground">{usage.connections}</span>
              </div>
              <p className="mt-1 text-[10px] leading-relaxed text-muted-foreground">{resolverDescription(usage.resolver)}</p>
            </div>
          ))}
        </div>
      </section>

      <section className="mt-5 border-t pt-4">
        <h4 className="text-xs font-semibold uppercase tracking-wide text-muted-foreground">
          File connections <span className="float-right tabular-nums">{connection.fileEdges.length}</span>
        </h4>
        <div className="mt-2 space-y-1.5">
          {connection.fileEdges.slice(0, 50).map((edge) => (
            <div key={graphEdgeId(edge)} className="rounded-lg border bg-background p-2.5">
              <button type="button" onClick={() => onLocate(edge.source)} className="block w-full truncate text-left font-mono text-[10px] hover:underline" title={edge.source}>
                {edge.source}
              </button>
              <div className="my-1 flex items-center gap-1 text-[9px] text-muted-foreground">
                <ChevronRight className="size-3" /> {resolverLabel(edge.resolver)}
              </div>
              <button type="button" onClick={() => onLocate(edge.target)} className="block w-full truncate text-left font-mono text-[10px] hover:underline" title={edge.target}>
                {edge.target}
              </button>
            </div>
          ))}
        </div>
      </section>
    </aside>
  )
}

function EntityEndpoint({ label, entity }: { label: string; entity?: ExplorerEntity }) {
  if (!entity) return null
  return (
    <div className="rounded-lg border bg-background p-3">
      <span className="text-[10px] font-semibold uppercase tracking-wide text-muted-foreground">{label}</span>
      <span className="mt-1 block break-all font-mono text-xs font-medium">{entity.path || "Project"}</span>
      <span className="mt-1 block text-[10px] text-muted-foreground">
        {entity.kind === "scope"
          ? `${entity.files} files · ${entity.graphFiles} connected`
          : `${entity.report.language} · ${formatCompact(entity.report.tokens)} tokens`}
      </span>
    </div>
  )
}

function ConnectionList({
  title,
  icon: Icon,
  edges,
  pathKey,
  onLocate,
}: {
  title: string
  icon: typeof ArrowDownToLine
  edges: GraphEdge[]
  pathKey: "source" | "target"
  onLocate: (path: string) => void
}) {
  return (
    <section className="mt-5 border-t pt-4">
      <h4 className="flex items-center gap-2 text-xs font-semibold uppercase tracking-wide text-muted-foreground">
        <Icon className="size-3.5" /> {title} <span className="ml-auto tabular-nums">{edges.length}</span>
      </h4>
      {edges.length === 0 ? (
        <p className="mt-2 text-xs text-muted-foreground">None resolved.</p>
      ) : (
        <div className="mt-2 space-y-1">
          {edges.map((edge) => {
            const path = edge[pathKey]
            return (
              <button
                key={graphEdgeId(edge)}
                type="button"
                onClick={() => onLocate(path)}
                className="block w-full rounded-md px-2 py-2 text-left hover:bg-accent focus-visible:bg-accent focus-visible:outline-none"
              >
                <span className="block truncate font-mono text-[11px]" title={path}>{path}</span>
                <span className="mt-0.5 block text-[10px] text-muted-foreground">{resolverLabel(edge.resolver)}</span>
              </button>
            )
          })}
        </div>
      )}
    </section>
  )
}

function FileFindings({ findings }: { findings: ExplorerFileInspection["findings"] }) {
  return (
    <section className="mt-5 border-t pt-4">
      <h4 className="flex items-center gap-2 text-xs font-semibold uppercase tracking-wide text-muted-foreground">
        <AlertTriangle className="size-3.5" /> Findings <span className="ml-auto tabular-nums">{findings.length}</span>
      </h4>
      <div className="mt-2 space-y-2">
        {findings.slice(0, 20).map((finding) => (
          <div key={`${finding.fingerprint}:${finding.primary_location.start_line}`} className="rounded-lg border p-2.5">
            <div className="flex flex-wrap items-center gap-1.5">
              <Badge variant={finding.severity === "error" ? "destructive" : "secondary"}>{finding.severity}</Badge>
              <span className="text-[10px] text-muted-foreground">{finding.kind} · line {finding.primary_location.start_line}</span>
            </div>
            <p className="mt-1.5 text-xs leading-relaxed">{finding.message}</p>
            {finding.metrics && Object.keys(finding.metrics).length > 0 ? (
              <p className="mt-1 font-mono text-[9px] text-muted-foreground">
                {Object.entries(finding.metrics).map(([name, value]) => `${name}=${value}`).join(" · ")}
              </p>
            ) : null}
          </div>
        ))}
      </div>
    </section>
  )
}

function DetailMetric({ label, value, detail }: { label: string; value: string; detail: string }) {
  return (
    <div className="rounded-lg bg-muted/60 px-3 py-2">
      <div className="text-[10px] uppercase tracking-wide text-muted-foreground">{label}</div>
      <div className="mt-0.5 text-base font-semibold tabular-nums">{value}</div>
      <div className="truncate text-[10px] text-muted-foreground">{detail}</div>
    </div>
  )
}

function MiniMetric({ label, value }: { label: string; value: number }) {
  return (
    <div className="rounded-lg bg-muted/60 px-2 py-2 text-center">
      <div className="text-base font-semibold tabular-nums">{formatNumber(value)}</div>
      <div className="text-[9px] uppercase tracking-wide text-muted-foreground">{label}</div>
    </div>
  )
}

function CopyPathButton({ path }: { path: string }) {
  const [copied, setCopied] = useState(false)
  useEffect(() => setCopied(false), [path])
  const copy = async () => {
    try {
      await navigator.clipboard.writeText(path)
      setCopied(true)
    } catch {
      setCopied(false)
    }
  }
  return (
    <Button type="button" size="icon-sm" variant="ghost" aria-label={copied ? "Path copied" : "Copy file path"} onClick={() => void copy()}>
      {copied ? <Check /> : <Copy />}
    </Button>
  )
}

function decorateLayout(
  view: ExplorerView,
  placements: ExplorerLayout,
  focus: string | null,
  selectedEntityId: string | null,
  selectedScopePath: string | null,
  selectedGroupId: string | null,
  selectedConnectionId: string | null,
  hoveredEntityId: string | null,
): { nodes: ExplorerFlowNode[]; edges: ExplorerEdge[] } {
  const mode = view.presentation
  const { dense, prominence, vertical } = placements

  const relatedIds = new Set<string>()
  if (selectedEntityId) {
    relatedIds.add(selectedEntityId)
    for (const connection of view.connections) {
      if (connection.source === selectedEntityId || connection.target === selectedEntityId) {
        relatedIds.add(connection.source)
        relatedIds.add(connection.target)
      }
    }
  } else if (selectedConnectionId) {
    const connection = view.connections.find((candidate) => candidate.id === selectedConnectionId)
    if (connection) {
      relatedIds.add(connection.source)
      relatedIds.add(connection.target)
    }
  }
  const selectedGroup = placements.groups.find((placement) =>
    selectedGroupId
      ? placement.group.id === selectedGroupId
      : selectedScopePath
        ? placement.group.path === selectedScopePath
        : false,
  )
  if (selectedGroup) {
    const memberIds = new Set(selectedGroup.group.members.map((member) => member.id))
    for (const id of memberIds) relatedIds.add(id)
    for (const connection of view.connections) {
      if (memberIds.has(connection.source) || memberIds.has(connection.target)) {
        relatedIds.add(connection.source)
        relatedIds.add(connection.target)
      }
    }
  }
  const focusedId = focus
    ? view.entities.find((entity) => entity.kind === "file" && entity.path === focus)?.id ?? null
    : null
  const connectedEdgeCount = selectedEntityId
    ? view.connections.filter((connection) => connection.source === selectedEntityId || connection.target === selectedEntityId).length
    : 0

  const groupNodes = placements.groups.map(({ group, position, width, height }) => {
    const memberIds = new Set(group.members.map((member) => member.id))
    const related = [...memberIds].some((id) => relatedIds.has(id))
    return {
      id: group.id,
      type: "explorerGroup",
      position,
      width,
      height,
      style: { width, height },
      selected: selectedGroupId === group.id || selectedScopePath === group.path,
      selectable: true,
      zIndex: 0,
      ariaLabel: group.kind === "relationship"
        ? `${group.label} ${group.name}, ${group.members.length} of ${group.totalMembers ?? group.members.length} files visible`
        : `${group.label} ${group.path || "Project root"}, ${group.members.length} visible items`,
      data: {
        group,
        related,
        dimmed: relatedIds.size > 0 && !related,
      },
    } satisfies ExplorerGroupFlowNode
  })
  const entityNodes = view.entities.map((entity) => {
    const placement = placements.entities.get(entity.id)!
    const entityProminence = prominence.get(entity.id)!
    return {
      id: entity.id,
      type: "explorerEntity",
      position: placement.position,
      width: placement.width,
      height: placement.height,
      parentId: placement.parentId,
      extent: placement.parentId ? "parent" : undefined,
      zIndex: placement.parentId ? 1 : 0,
      sourcePosition: vertical ? Position.Bottom : Position.Right,
      targetPosition: vertical ? Position.Top : Position.Left,
      selected: selectedEntityId === entity.id,
      ariaLabel: entityAriaLabel(entity, entityProminence),
      data: {
        entity,
        focused: focusedId === entity.id,
        related: relatedIds.has(entity.id),
        dimmed: relatedIds.size > 0 && !relatedIds.has(entity.id),
        vertical,
        prominence: entityProminence,
        typeFocused: mode === "type" && entity.path === focus,
        width: placement.width,
        height: placement.height,
      },
    } satisfies ExplorerNode
  })
  const edges = view.connections.map((connection) => {
    const selected = selectedConnectionId === connection.id
    const connected = selectedEntityId === connection.source || selectedEntityId === connection.target
    const focused = focusedId === connection.source || focusedId === connection.target
    const hovered = hoveredEntityId === connection.source || hoveredEntityId === connection.target
    const highlighted = selected || connected || focused || hovered
    const muted = relatedIds.size > 0 && !selected && !connected && !hovered
    const explicitType = connection.relation === "extends"
      || connection.relation === "implements"
      || connection.relation === "embeds"
    const semanticImport = mode === "type" && !explicitType
    const color = selected
      ? "var(--chart-3)"
      : mode === "type" && explicitType
        ? "var(--primary)"
        : highlighted
          ? "var(--foreground)"
          : "var(--muted-foreground)"
    const baseWidth = 1.1 + Math.min(2.4, Math.log2(Math.max(1, connection.count)) * 0.7)
    const idleOpacity = dense ? 0.1 : mode === "architecture" ? 0.42 : 0.24
    const showIdleAggregateLabel = !dense
      && mode === "architecture"
      && view.connections.length <= 12
      && connection.count > 1
    return {
      id: connection.id,
      source: connection.source,
      target: connection.target,
      type: "explorerConnection",
      selected,
      ariaLabel: `${relationLabel(connection.relation)} from ${connection.source} to ${connection.target}, ${connection.count} file connections`,
      markerEnd: highlighted || !dense
        ? { type: MarkerType.ArrowClosed, color, width: 15, height: 15 }
        : undefined,
      style: {
        stroke: color,
        strokeWidth: selected
          ? Math.max(2.8, baseWidth)
          : highlighted
            ? Math.max(2, baseWidth)
            : dense
              ? Math.min(1, baseWidth)
              : baseWidth,
        strokeDasharray: semanticImport ? "7 5" : undefined,
        opacity: semanticImport
          ? selected || hovered ? 0.85 : 0.42
          : muted
            ? (dense ? 0.025 : 0.06)
            : highlighted
              ? 0.94
              : idleOpacity,
      },
      zIndex: selected ? 3 : highlighted ? 2 : 0,
      data: {
        connection,
        showLabel: selected
          || showIdleAggregateLabel
          || Boolean(selectedEntityId && connected && connectedEdgeCount <= 6),
      },
    } satisfies ExplorerEdge
  })
  return { nodes: [...groupNodes, ...entityNodes], edges }
}

function entityAriaLabel(entity: ExplorerEntity, prominence: GraphProminence): string {
  if (entity.kind === "scope") {
    return `${entity.path || "Project"}, ${entity.files} files, ${entity.graphFiles} topology files, ${entity.fanIn} incoming and ${entity.fanOut} outgoing relationships`
  }
  const graph = entity.graphFile
  const reach = prominence.level === "standard" ? "" : `, ${prominence.label}: ${prominence.reason}`
  return `${entity.path}, ${entity.report.language}, ${categoryLabel(entity.category)}${graph ? `, ${graph.fan_in} incoming and ${graph.fan_out} outgoing relationships` : ""}${reach}`
}

function relationLabel(relation: ExplorerConnection["relation"]): string {
  const labels: Record<ExplorerConnection["relation"], string> = {
    imports: "imports",
    includes: "includes",
    "declares-module": "declares module",
    "imports-package": "imports package",
    extends: "extends",
    implements: "implements",
    embeds: "embeds",
    mixed: "mixed relationships",
  }
  return labels[relation]
}

function resolverLabel(resolver: string): string {
  return resolver
    .split("-")
    .map((part) => {
      if (part === "tsconfig") return "tsconfig"
      if (part === "psr") return "PSR"
      if (part === "php") return "PHP"
      if (part === "go") return "Go"
      if (part === "rust") return "Rust"
      return part.charAt(0).toUpperCase() + part.slice(1)
    })
    .join(" ")
}

function resolverDescription(resolver: string): string {
  const descriptions: Record<string, string> = {
    relative: "A relative JavaScript or TypeScript specifier resolved against the importing file.",
    "tsconfig-paths": "A tsconfig or jsconfig paths mapping resolved this alias.",
    "tsconfig-base-url": "A tsconfig or jsconfig baseUrl resolved this non-relative import.",
    "heuristic-alias": "RepoScout's conventional @/ alias fallback resolved this import.",
    "package-imports": "The importing package's package.json imports map resolved this private alias.",
    "package-exports": "A local package.json exports map resolved this public package path.",
    "package-subpath": "A local workspace package subpath resolved directly to this source file.",
    "package-entrypoint": "A local workspace package entrypoint resolved to this file.",
    "package-index": "A local workspace package directory resolved through its index file.",
    "python-relative": "A dotted relative Python import resolved within the current package.",
    "python-absolute": "An unambiguous repository-absolute Python module path resolved to this file.",
    "python-src-root": "A conventional Python src root resolved this absolute module path.",
    "composer-psr-4": "A Composer PSR-4 autoload mapping resolved this PHP namespace.",
    "composer-psr-0": "A Composer PSR-0 autoload mapping resolved this legacy PHP class name.",
    "php-include": "A static PHP include or require expression resolved to this file.",
    "php-namespace-heuristic": "A conventional PHP src, app, or lib namespace layout resolved this target.",
    "rust-mod": "A Rust mod declaration resolved to its module source file.",
    "rust-path": "A Rust #[path] module attribute resolved to its explicit source file.",
    "rust-use": "A crate, self, super, or unambiguous local Rust use path resolved to a module file.",
    "rust-workspace": "A local Cargo package or library crate name resolved this Rust use path.",
    "go-module": "A local go.mod module prefix resolved the imported Go package; its stable representative file anchors the package edge.",
    "go-relative": "A relative Go package path resolved to its stable representative file.",
    "symbol-extends": "An explicit class, interface, or trait base resolved to an unambiguous repository symbol.",
    "symbol-implements": "An explicit interface or trait implementation resolved to an unambiguous repository symbol.",
    "symbol-embeds": "An explicit Go interface or struct embedding resolved to an unambiguous repository symbol.",
  }
  return descriptions[resolver] ?? `RepoScout resolved this relationship using the ${resolverLabel(resolver)} strategy.`
}

function graphEdgeId(edge: GraphEdge): string {
  return `${edge.source}→${edge.target}:${edge.resolver}`
}

function categoryLabel(category: ExplorerFileSummary["category"]): string {
  const labels: Record<ExplorerFileSummary["category"], string> = {
    source: "Source",
    test: "Test",
    config: "Config",
    schema: "Schema",
    entrypoint: "Entrypoint",
    generated: "Generated",
  }
  return labels[category]
}

function scopeKindLabel(kind: Extract<ExplorerEntity, { kind: "scope" }>["scopeKind"]): string {
  const labels = { project: "Project", package: "Package", area: "Area", directory: "Directory" }
  return labels[kind]
}

function shortDate(value?: string): string {
  return value ? value.slice(0, 10) : "?"
}

function miniMapEntityColor(entity: ExplorerEntity): string {
  if (entity.kind === "scope") {
    if (entity.external) return "#64748b"
    if (entity.riskFiles > 0) return "#fb7185"
    if (entity.findings > 0) return "#fbbf24"
    return "#2dd4bf"
  }
  if (entity.category === "generated") return "#64748b"
  const language = entity.report.language.toLowerCase()
  if (language.includes("typescript") || language === "tsx") return "#60a5fa"
  if (language.includes("javascript") || language === "jsx") return "#fde047"
  if (language.includes("python")) return "#38bdf8"
  if (language === "php") return "#c084fc"
  if (language === "rust") return "#fb923c"
  if (language === "go") return "#22d3ee"
  return "#94a3b8"
}

function scopeColor(scope: Extract<ExplorerEntity, { kind: "scope" }>): string {
  if (scope.external) return "var(--muted-foreground)"
  if (scope.riskFiles > 0) return "var(--chart-5)"
  if (scope.findings > 0) return "var(--chart-3)"
  return "var(--chart-2)"
}

function languageColor(language: string): string {
  const normalized = language.toLowerCase()
  if (normalized.includes("typescript") || normalized === "tsx") return "var(--chart-3)"
  if (normalized.includes("javascript") || normalized === "jsx") return "var(--chart-4)"
  if (normalized.includes("python")) return "var(--chart-1)"
  if (normalized === "php") return "var(--chart-5)"
  if (normalized === "rust") return "oklch(0.68 0.17 50)"
  if (normalized === "go") return "oklch(0.72 0.14 215)"
  return "var(--chart-2)"
}
