import { useCallback, useEffect, useMemo, useState } from "react"
import { useLocation, useNavigate } from "react-router"
import { AlertTriangle, Network, RotateCcw } from "lucide-react"
import type { NodeMouseHandler } from "@xyflow/react"
import "@xyflow/react/dist/style.css"

import { Button } from "@/components/ui/button"
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card"
import { Skeleton } from "@/components/ui/skeleton"
import { useRepositoryGraph } from "@/hooks/use-repository-graph"
import {
  buildRepositoryGraphExplorer,
  type ExplorerConnection,
  type ExplorerFileInspection,
  type ExplorerNeighborhoodPresentation,
  type ExplorerView,
  type RepositoryGraphExplorer,
} from "@/lib/graph-explorer-model"
import type { GraphDirection } from "@/lib/graph-data"
import {
  isDenseExplorerView,
  layoutExplorerView,
} from "@/lib/graph-explorer-layout"
import {
  GRAPH_ROOT_ROUTE,
  graphRoutePath,
  parseGraphRoute,
  type GraphRoute,
  type GraphRouteDepth,
} from "@/lib/graph-routes"
import type { DependencyGraph, ScanReport } from "@/lib/types"
import { GraphExplorerView } from "@/components/repository-graph-view"
import { buildCanvasLegend } from "@/components/repository-graph-legend"
import { decorateLayout } from "@/components/repository-graph-layout"
import type {
  ExplorerEdge,
  ExplorerFlowNode,
  ExplorerMode,
  GraphSelection,
  NavigateGraphRoute,
} from "@/components/repository-graph-types"

interface RepositoryGraphProps {
  revision: number
  report: ScanReport
}

interface RoutedState<T> {
  routeKey: string
  value: T
}

export function RepositoryGraph({ revision, report }: RepositoryGraphProps) {
  const location = useLocation()
  const navigate = useNavigate()
  const route = useMemo(
    () => parseGraphRoute(location.pathname, location.search),
    [location.pathname, location.search]
  )
  const navigateGraph = useCallback<NavigateGraphRoute>(
    (next, options) => {
      navigate(graphRoutePath(next), { replace: options?.replace })
    },
    [navigate]
  )
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
          <CardTitle className="flex items-center gap-2">
            <AlertTriangle className="size-4" /> Graph unavailable
          </CardTitle>
          <CardDescription>{request.error}</CardDescription>
        </CardHeader>
        <CardContent>
          <Button variant="outline" onClick={request.retry}>
            <RotateCcw /> Try again
          </Button>
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
            Repository topology supports Rust, Python, JavaScript,
            TypeScript/TSX, Go, and PHP.
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
        <CardTitle className="flex items-center gap-2">
          <Network className="size-4 animate-pulse" /> Building repository graph
        </CardTitle>
        <CardDescription>
          This analysis runs only when the Graph tab is opened.
        </CardDescription>
      </CardHeader>
      <CardContent className="grid gap-4 xl:grid-cols-[minmax(0,1fr)_28rem]">
        <Skeleton className="h-[52rem] w-full" />
        <Skeleton className="h-[52rem] w-full" />
      </CardContent>
    </Card>
  )
}

function selectionFromRoute(
  route: GraphRoute,
  routedFile: ExplorerFileInspection | null
): GraphSelection {
  if (route.kind !== "file" || !routedFile) return null
  return { kind: "file", path: route.focus }
}

interface ExplorerRouteState {
  routedFile: ExplorerFileInspection | null
  validFileRoute: boolean
  validScopeRoute: boolean
  mode: ExplorerMode
  focus: string | null
  scopePath: string
  direction: GraphDirection
  depth: GraphRouteDepth
  neighborhoodPresentation: ExplorerNeighborhoodPresentation
  routeKey: string
  routeSelection: GraphSelection
  routeSearch: string
}

function buildExplorerRouteState(
  route: GraphRoute,
  explorer: RepositoryGraphExplorer,
  reportFiles: ScanReport["files"]
): ExplorerRouteState {
  const routedFile =
    route.kind === "file" ? explorer.inspectFile(route.focus) : null
  const validFileRoute = route.kind === "file" && Boolean(routedFile?.graph)
  const validScopeRoute =
    route.kind !== "architecture" ||
    !route.scopePath ||
    reportFiles.some((file) => file.path.startsWith(`${route.scopePath}/`))
  const mode: ExplorerMode = validFileRoute ? "neighborhood" : "architecture"
  const focus = validFileRoute && route.kind === "file" ? route.focus : null
  const scopePath =
    route.kind === "architecture"
      ? validScopeRoute
        ? route.scopePath
        : ""
      : routedFile
        ? explorer.parentScope(route.focus)
        : ""
  return {
    routedFile,
    validFileRoute,
    validScopeRoute,
    mode,
    focus,
    scopePath,
    direction: route.kind === "file" ? route.direction : "both",
    depth: route.kind === "file" ? route.depth : 2,
    neighborhoodPresentation:
      route.kind === "file" ? route.presentation : "auto",
    routeKey: graphRoutePath(route),
    routeSelection: selectionFromRoute(route, routedFile),
    routeSearch: route.kind === "file" ? route.focus : "",
  }
}

interface SelectionProjection {
  selectedConnection: ExplorerConnection | null
  selectedEntityId: string | null
  selectedScopePath: string | null
  selectedGroupId: string | null
}

function projectSelection(
  selection: GraphSelection,
  display: ExplorerView
): SelectionProjection {
  const selectedConnection =
    selection?.kind === "connection"
      ? (display.connections.find(
          (connection) => connection.id === selection.id
        ) ?? null)
      : null
  const selectedEntityId =
    selection?.kind === "file" || selection?.kind === "scope"
      ? (display.entities.find(
          (entity) =>
            entity.kind === selection.kind && entity.path === selection.path
        )?.id ?? null)
      : null
  return {
    selectedConnection,
    selectedEntityId,
    selectedScopePath: selection?.kind === "scope" ? selection.path : null,
    selectedGroupId: selection?.kind === "group" ? selection.id : null,
  }
}

function fitViewMinimumZoom(
  denseView: boolean,
  typeFocused: boolean,
  entityCount: number
): number {
  if (denseView) return 0.42
  if (typeFocused) return 0.32
  return entityCount > 18 ? 0.28 : 0.16
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
  const explorer = useMemo(
    () => buildRepositoryGraphExplorer(graph, report),
    [graph, report]
  )
  const routeState = useMemo(
    () => buildExplorerRouteState(route, explorer, report.files),
    [explorer, report.files, route]
  )
  const {
    routedFile,
    validFileRoute,
    validScopeRoute,
    mode,
    focus,
    scopePath,
    direction,
    depth,
    neighborhoodPresentation,
    routeKey,
    routeSelection,
    routeSearch,
  } = routeState
  const [selectionState, setSelectionState] = useState<
    RoutedState<GraphSelection>
  >(() => ({ routeKey, value: routeSelection }))
  const selection =
    selectionState.routeKey === routeKey ? selectionState.value : routeSelection
  const setSelection = useCallback(
    (value: GraphSelection) => setSelectionState({ routeKey, value }),
    [routeKey]
  )
  const [searchState, setSearchState] = useState<RoutedState<string>>(() => ({
    routeKey,
    value: routeSearch,
  }))
  const search =
    searchState.routeKey === routeKey ? searchState.value : routeSearch
  const setSearch = useCallback(
    (value: string) => setSearchState({ routeKey, value }),
    [routeKey]
  )
  const setFileStateForRoute = useCallback(
    (path: string, nextRoute: GraphRoute) => {
      const nextRouteKey = graphRoutePath(nextRoute)
      setSelectionState({
        routeKey: nextRouteKey,
        value: { kind: "file", path },
      })
      setSearchState({ routeKey: nextRouteKey, value: path })
    },
    []
  )
  const [searchOpen, setSearchOpen] = useState(false)
  const [expanded, setExpanded] = useState(false)
  const [hoveredEntityId, setHoveredEntityId] = useState<string | null>(null)

  const architectureView = useMemo(
    () => explorer.view(scopePath),
    [explorer, scopePath]
  )
  const display = useMemo(
    () =>
      mode === "neighborhood" && focus
        ? explorer.neighborhood(
            focus,
            direction,
            depth,
            neighborhoodPresentation
          )
        : architectureView,
    [
      architectureView,
      depth,
      direction,
      explorer,
      focus,
      mode,
      neighborhoodPresentation,
    ]
  )
  const matches = useMemo(() => explorer.search(search), [explorer, search])
  const {
    selectedConnection,
    selectedEntityId,
    selectedScopePath,
    selectedGroupId,
  } = useMemo(() => projectSelection(selection, display), [display, selection])
  const placements = useMemo(
    () => layoutExplorerView(display, display.presentation),
    [display]
  )
  const layout = useMemo(
    () =>
      decorateLayout(display, placements, {
        focus,
        selectedEntityId,
        selectedScopePath,
        selectedGroupId,
        selectedConnectionId: selectedConnection?.id ?? null,
        hoveredEntityId,
      }),
    [
      display,
      focus,
      hoveredEntityId,
      placements,
      selectedConnection?.id,
      selectedEntityId,
      selectedGroupId,
      selectedScopePath,
    ]
  )
  const legend = useMemo(
    () => buildCanvasLegend(display, placements.prominence),
    [display, placements]
  )
  const visibleGroups = layout.nodes.filter(
    (node) => node.type === "explorerGroup"
  ).length
  const typeFocused = display.presentation === "type"
  const typeFocusAvailable = Boolean(
    focus && explorer.inspectFile(focus)?.graph?.symbolRelations.length
  )
  const denseView = !typeFocused && isDenseExplorerView(display)
  const initialFitMinZoom = fitViewMinimumZoom(
    denseView,
    typeFocused,
    display.entities.length
  )

  const openScope = useCallback(
    (path: string) => {
      setSelection(null)
      setSearchOpen(false)
      onRouteChange({ kind: "architecture", scopePath: path })
    },
    [onRouteChange, setSelection]
  )

  const locateFile = useCallback(
    (path: string) => {
      const nextRoute: GraphRoute = {
        kind: "architecture",
        scopePath: explorer.parentScope(path),
      }
      setFileStateForRoute(path, nextRoute)
      setSearchOpen(false)
      onRouteChange(nextRoute)
    },
    [explorer, onRouteChange, setFileStateForRoute]
  )

  const exploreFile = useCallback(
    (
      path: string,
      nextDirection: GraphDirection = "both",
      presentation: ExplorerNeighborhoodPresentation = "auto"
    ) => {
      const nextRoute: GraphRoute = {
        kind: "file",
        focus: path,
        direction: nextDirection,
        depth: 2,
        presentation,
      }
      setFileStateForRoute(path, nextRoute)
      setSearchOpen(false)
      onRouteChange(nextRoute)
    },
    [onRouteChange, setFileStateForRoute]
  )

  const updateNeighborhoodRoute = useCallback(
    (updates: {
      direction?: GraphDirection
      depth?: GraphRouteDepth
      presentation?: ExplorerNeighborhoodPresentation
    }) => {
      if (route.kind !== "file" || !validFileRoute) return
      onRouteChange({ ...route, ...updates })
    },
    [onRouteChange, route, validFileRoute]
  )

  useEffect(() => {
    if (route.kind === "architecture") {
      if (!validScopeRoute) onRouteChange(GRAPH_ROOT_ROUTE, { replace: true })
      return
    }
    if (validFileRoute) return
    onRouteChange(
      {
        kind: "architecture",
        scopePath: routedFile ? explorer.parentScope(route.focus) : "",
      },
      { replace: true }
    )
  }, [
    explorer,
    onRouteChange,
    route,
    routedFile,
    validFileRoute,
    validScopeRoute,
  ])

  const submitSearch = (event: React.FormEvent) => {
    event.preventDefault()
    const exact = report.files.find((file) => file.path === search.trim())
    const match = exact ? explorer.inspectFile(exact.path)?.file : matches[0]
    if (match) locateFile(match.path)
  }

  const onNodeClick = useCallback<NodeMouseHandler<ExplorerFlowNode>>(
    (_, node) => {
      if (node.type === "explorerGroup") {
        setSelection(
          node.data.group.kind === "relationship"
            ? { kind: "group", id: node.data.group.id }
            : { kind: "scope", path: node.data.group.path }
        )
        return
      }
      const entity = node.data.entity
      setSelection({ kind: entity.kind, path: entity.path })
    },
    [setSelection]
  )
  const onNodeDoubleClick = useCallback<NodeMouseHandler<ExplorerFlowNode>>(
    (_, node) => {
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
    },
    [exploreFile, openScope, setSelection]
  )
  const onEdgeClick = useCallback(
    (_: React.MouseEvent, edge: ExplorerEdge) => {
      if (edge.data?.connection) {
        setSelection({ kind: "connection", id: edge.data.connection.id })
      }
    },
    [setSelection]
  )
  const onNodeMouseEnter = useCallback<NodeMouseHandler<ExplorerFlowNode>>(
    (_, node) => {
      setHoveredEntityId(node.type === "explorerEntity" ? node.id : null)
    },
    []
  )
  const onNodeMouseLeave = useCallback(() => setHoveredEntityId(null), [])

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
  }, [
    architectureView.breadcrumbs,
    expanded,
    explorer,
    focus,
    mode,
    openScope,
    searchOpen,
    selection,
    setSelection,
  ])

  return (
    <GraphExplorerView
      model={{
        graph,
        explorer,
        architectureView,
        display,
        layout,
        legend,
        selection,
        selectedConnection,
        expanded,
        mode,
        focus,
        scopePath,
        direction,
        depth,
        typeFocused,
        typeFocusAvailable,
        denseView,
        initialFitMinZoom,
        visibleGroups,
        search,
        searchOpen,
        matches,
      }}
      actions={{
        setExpanded,
        setSearch,
        setSearchOpen,
        submitSearch,
        updateNeighborhoodRoute,
        openScope,
        locateFile,
        exploreFile,
        setSelection,
        onNodeClick,
        onNodeDoubleClick,
        onNodeMouseEnter,
        onNodeMouseLeave,
        onEdgeClick,
      }}
    />
  )
}
