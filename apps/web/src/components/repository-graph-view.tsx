import type {
  Dispatch,
  FormEventHandler,
  MouseEvent as ReactMouseEvent,
  SetStateAction,
} from "react"
import {
  Background,
  BackgroundVariant,
  Controls,
  MiniMap,
  Panel,
  ReactFlow,
  type NodeMouseHandler,
} from "@xyflow/react"
import { GitBranch, Layers3, Maximize2, Minimize2, Search } from "lucide-react"

import { GraphDetails } from "@/components/repository-graph-details"
import {
  edgeTypes,
  miniMapNodeColor,
  nodeTypes,
} from "@/components/repository-graph-flow-types"
import type { CanvasLegendData } from "@/components/repository-graph-legend"
import {
  CanvasLegend,
  GraphBreadcrumbs,
  GraphMetric,
  LabeledSelect,
} from "@/components/repository-graph-controls"
import type {
  ExplorerEdge,
  ExplorerFlowNode,
  ExplorerMode,
  GraphSelection,
} from "@/components/repository-graph-types"
import {
  categoryLabel,
  languageColor,
} from "@/components/repository-graph-visuals"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
} from "@/components/ui/card"
import type {
  ExplorerConnection,
  ExplorerFileSummary,
  ExplorerNeighborhoodPresentation,
  ExplorerView,
  RepositoryGraphExplorer,
} from "@/lib/graph-explorer-model"
import {
  graphFileName,
  graphProminence,
  type GraphDirection,
} from "@/lib/graph-data"
import type { DecoratedExplorerLayout } from "@/components/repository-graph-layout"
import { parseGraphRouteDepth, type GraphRouteDepth } from "@/lib/graph-routes"
import { formatCompact } from "@/lib/format"
import type { DependencyGraph } from "@/lib/types"
import { cn } from "@/lib/utils"

interface GraphExplorerViewModel {
  graph: DependencyGraph
  explorer: RepositoryGraphExplorer
  architectureView: ExplorerView
  display: ExplorerView
  layout: DecoratedExplorerLayout
  legend: CanvasLegendData
  selection: GraphSelection
  selectedConnection: ExplorerConnection | null
  expanded: boolean
  mode: ExplorerMode
  focus: string | null
  scopePath: string
  direction: GraphDirection
  depth: GraphRouteDepth
  typeFocused: boolean
  typeFocusAvailable: boolean
  denseView: boolean
  initialFitMinZoom: number
  visibleGroups: number
  search: string
  searchOpen: boolean
  matches: ExplorerFileSummary[]
}

interface GraphExplorerViewActions {
  setExpanded: Dispatch<SetStateAction<boolean>>
  setSearch: (value: string) => void
  setSearchOpen: Dispatch<SetStateAction<boolean>>
  submitSearch: FormEventHandler<HTMLFormElement>
  updateNeighborhoodRoute: (updates: {
    direction?: GraphDirection
    depth?: GraphRouteDepth
    presentation?: ExplorerNeighborhoodPresentation
  }) => void
  openScope: (path: string) => void
  locateFile: (path: string) => void
  exploreFile: (
    path: string,
    direction?: GraphDirection,
    presentation?: ExplorerNeighborhoodPresentation
  ) => void
  setSelection: (selection: GraphSelection) => void
  onNodeClick: NodeMouseHandler<ExplorerFlowNode>
  onNodeDoubleClick: NodeMouseHandler<ExplorerFlowNode>
  onNodeMouseEnter: NodeMouseHandler<ExplorerFlowNode>
  onNodeMouseLeave: () => void
  onEdgeClick: (event: ReactMouseEvent, edge: ExplorerEdge) => void
}

export function GraphExplorerView({
  model,
  actions,
}: {
  model: GraphExplorerViewModel
  actions: GraphExplorerViewActions
}) {
  return (
    <div className="space-y-4">
      <GraphSummary graph={model.graph} />
      <ExpandedBackdrop expanded={model.expanded} />
      <GraphWorkspace model={model} actions={actions} />
    </div>
  )
}

function GraphSummary({ graph }: { graph: DependencyGraph }) {
  const hubFiles = graph.files.filter(
    (file) => graphProminence(file).level === "hub"
  ).length
  const topDepended = graph.top_depended[0] ?? null
  const largestCycle = graph.cycles.reduce(
    (largest, cycle) => Math.max(largest, cycle.length),
    0
  )
  return (
    <section className="grid grid-cols-2 gap-3 lg:grid-cols-3 xl:grid-cols-6">
      <GraphMetric
        label="First-class files"
        value={graph.nodes}
        detail={graph.languages.join(", ")}
      />
      <GraphMetric
        label="Relationships"
        value={graph.edges + (graph.symbol_edges?.length ?? 0)}
        detail={`${graph.edges} file · ${graph.symbol_edges?.length ?? 0} type`}
      />
      <GraphMetric
        label="Hub files"
        value={hubFiles}
        detail={
          topDepended
            ? `${graphFileName(topDepended.path)} leads · ${topDepended.fan_in} dependents`
            : "no wide-reach files"
        }
      />
      <GraphMetric
        label="Cycles"
        value={graph.cycles.length}
        detail={
          largestCycle > 0
            ? `largest loops through ${largestCycle} files`
            : "no circular dependencies"
        }
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
  )
}

function ExpandedBackdrop({ expanded }: { expanded: boolean }) {
  if (!expanded) return null
  return (
    <div
      className="fixed inset-0 z-40 bg-background/80 backdrop-blur-sm"
      aria-hidden="true"
    />
  )
}

function GraphWorkspace({
  model,
  actions,
}: {
  model: GraphExplorerViewModel
  actions: GraphExplorerViewActions
}) {
  return (
    <Card
      data-expanded={model.expanded}
      className={cn(
        "repository-graph-workspace gap-4 overflow-hidden py-4",
        model.expanded &&
          "fixed inset-2 z-50 gap-3 rounded-xl bg-card py-3 shadow-2xl sm:inset-4"
      )}
    >
      <WorkspaceHeader model={model} actions={actions} />
      <WorkspaceContent model={model} actions={actions} />
    </Card>
  )
}

function WorkspaceHeader({
  model,
  actions,
}: {
  model: GraphExplorerViewModel
  actions: GraphExplorerViewActions
}) {
  return (
    <CardHeader className="gap-4 px-4 sm:px-5">
      <WorkspaceHeading model={model} actions={actions} />
      <GraphToolbar model={model} actions={actions} />
      <GraphStatus model={model} />
    </CardHeader>
  )
}

function WorkspaceHeading({
  model,
  actions,
}: {
  model: GraphExplorerViewModel
  actions: GraphExplorerViewActions
}) {
  const { architectureView, expanded, focus, graph, mode, typeFocused } = model
  return (
    <div className="flex flex-wrap items-start justify-between gap-3">
      <div className="min-w-0">
        <GraphBreadcrumbs
          view={architectureView}
          mode={mode}
          focus={focus}
          onOpen={actions.openScope}
        />
        <CardDescription className="mt-1">
          {typeFocused
            ? "Type structure keeps the selected symbol central, groups explicit type relations, and quiets ordinary imports."
            : "Hover a node to trace its links, click to inspect, double-click to open. Scroll pans; pinch or Ctrl+scroll zooms."}
        </CardDescription>
      </div>
      <div className="flex flex-wrap items-center justify-end gap-2">
        {graph.config_files?.slice(0, 3).map((path) => (
          <Badge key={path} variant="outline">
            {path}
          </Badge>
        ))}
        {(graph.config_files?.length ?? 0) > 3 ? (
          <Badge variant="outline">
            +{graph.config_files!.length - 3} configs
          </Badge>
        ) : null}
        {(graph.config_errors ?? 0) > 0 ? (
          <Badge variant="destructive">
            {graph.config_errors} config errors
          </Badge>
        ) : null}
        {(graph.parse_errors ?? 0) > 0 ? (
          <Badge variant="secondary">{graph.parse_errors} parse errors</Badge>
        ) : null}
        <Button
          type="button"
          size="sm"
          variant="outline"
          aria-label={
            expanded ? "Restore graph workspace" : "Expand graph workspace"
          }
          aria-pressed={expanded}
          onClick={() => actions.setExpanded((current) => !current)}
        >
          {expanded ? <Minimize2 /> : <Maximize2 />}
          {expanded ? "Restore" : "Expand"}
        </Button>
      </div>
    </div>
  )
}

function GraphToolbar({
  model,
  actions,
}: {
  model: GraphExplorerViewModel
  actions: GraphExplorerViewActions
}) {
  const {
    depth,
    direction,
    focus,
    mode,
    scopePath,
    typeFocusAvailable,
    typeFocused,
  } = model
  return (
    <div
      className={cn(
        "grid gap-3",
        mode === "neighborhood" && typeFocusAvailable
          ? "lg:grid-cols-[minmax(16rem,1fr)_auto_auto_auto_auto]"
          : "lg:grid-cols-[minmax(16rem,1fr)_auto_auto_auto]"
      )}
    >
      <GraphSearch model={model} actions={actions} />
      <LabeledSelect
        label="Direction"
        value={direction}
        disabled={mode !== "neighborhood" || typeFocused}
        onChange={(value) =>
          actions.updateNeighborhoodRoute({ direction: value })
        }
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
        onChange={(value) =>
          actions.updateNeighborhoodRoute({
            depth: parseGraphRouteDepth(value),
          })
        }
        options={[
          ["1", "1 hop"],
          ["2", "2 hops"],
          ["3", "3 hops"],
        ]}
      />
      <TypeStructureButton model={model} actions={actions} />
      <Button
        type="button"
        variant="outline"
        disabled={mode === "architecture" && !scopePath}
        onClick={() =>
          mode === "neighborhood"
            ? actions.openScope(
                focus ? model.explorer.parentScope(focus) : scopePath
              )
            : actions.openScope("")
        }
      >
        <Layers3 /> {mode === "neighborhood" ? "Architecture" : "Project"}
      </Button>
    </div>
  )
}

function GraphSearch({
  model,
  actions,
}: {
  model: GraphExplorerViewModel
  actions: GraphExplorerViewActions
}) {
  const { matches, search, searchOpen, selection } = model
  return (
    <form
      className="relative flex min-w-0 gap-2"
      onSubmit={actions.submitSearch}
    >
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
          onChange={(event) => {
            actions.setSearch(event.target.value)
            actions.setSearchOpen(true)
          }}
          onFocus={() => actions.setSearchOpen(true)}
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
                aria-selected={
                  selection?.kind === "file" && selection.path === file.path
                }
                onMouseDown={(event) => event.preventDefault()}
                onClick={() => actions.locateFile(file.path)}
                className="flex w-full items-center gap-3 rounded-md px-3 py-2 text-left hover:bg-accent focus-visible:bg-accent focus-visible:outline-none"
              >
                <span
                  className="size-2 shrink-0 rounded-full"
                  style={{ background: languageColor(file.report.language) }}
                />
                <span className="min-w-0 flex-1">
                  <span className="block truncate font-mono text-xs font-medium">
                    {file.path}
                  </span>
                  <span className="block text-[10px] text-muted-foreground">
                    {categoryLabel(file.category)} · {file.report.language} ·{" "}
                    {formatCompact(file.report.tokens)} tokens
                  </span>
                </span>
              </button>
            ))}
          </div>
        ) : null}
      </div>
      <Button type="submit" variant="outline">
        Locate
      </Button>
    </form>
  )
}

function TypeStructureButton({
  model,
  actions,
}: {
  model: GraphExplorerViewModel
  actions: GraphExplorerViewActions
}) {
  const { mode, typeFocusAvailable, typeFocused } = model
  if (mode !== "neighborhood" || !typeFocusAvailable) return null
  return (
    <Button
      type="button"
      variant={typeFocused ? "secondary" : "outline"}
      aria-label={
        typeFocused ? "Show full neighborhood" : "Show type structure"
      }
      onClick={() =>
        actions.updateNeighborhoodRoute({
          presentation: typeFocused ? "full" : "type",
        })
      }
    >
      <GitBranch /> {typeFocused ? "Full neighborhood" : "Type structure"}
    </Button>
  )
}

function GraphStatus({ model }: { model: GraphExplorerViewModel }) {
  const { denseView, display, mode, typeFocused, visibleGroups } = model
  return (
    <div className="flex flex-wrap items-center gap-2 text-xs text-muted-foreground">
      <Badge variant="secondary">
        Showing {display.entities.length} of {display.totalEntities}{" "}
        {mode === "architecture" ? "items" : "files"}
      </Badge>
      <span>{display.connections.length} visible relationships</span>
      <span>
        {display.scope.graphFiles} of {display.scope.files} files participate in
        topology
      </span>
      {visibleGroups > 0 ? (
        <Badge variant="outline">
          {visibleGroups}{" "}
          {typeFocused
            ? "relationship"
            : mode === "architecture"
              ? "architecture"
              : "path-based"}{" "}
          {visibleGroups === 1 ? "group" : "groups"}
        </Badge>
      ) : null}
      {typeFocused ? (
        <Badge variant="outline">
          Type structure · direct declared relationships
        </Badge>
      ) : null}
      {denseView ? (
        <Badge variant="outline">
          Dense view · select a node to isolate direct links
        </Badge>
      ) : null}
      {display.truncated ? <Badge variant="outline">bounded view</Badge> : null}
    </div>
  )
}

function WorkspaceContent({
  model,
  actions,
}: {
  model: GraphExplorerViewModel
  actions: GraphExplorerViewActions
}) {
  return (
    <CardContent
      className={cn(
        "grid min-h-0 gap-4 px-4 sm:px-5 xl:grid-cols-[minmax(0,1fr)_28rem]",
        model.expanded &&
          "flex-1 overflow-auto lg:grid-cols-[minmax(0,1fr)_30rem] lg:overflow-hidden xl:grid-cols-[minmax(0,1fr)_30rem]"
      )}
    >
      <GraphCanvas model={model} actions={actions} />
      <GraphDetails
        explorer={model.explorer}
        view={model.display}
        selection={model.selection}
        connection={model.selectedConnection}
        expanded={model.expanded}
        onLocate={actions.locateFile}
        onExplore={actions.exploreFile}
      />
    </CardContent>
  )
}

function GraphCanvas({
  model,
  actions,
}: {
  model: GraphExplorerViewModel
  actions: GraphExplorerViewActions
}) {
  const {
    depth,
    direction,
    display,
    expanded,
    focus,
    initialFitMinZoom,
    layout,
    legend,
    mode,
    scopePath,
    typeFocused,
  } = model
  return (
    <div
      className={cn(
        "repository-graph-flow h-[52rem] overflow-hidden rounded-xl border bg-muted/20",
        expanded && "h-[56dvh] min-h-80 lg:h-full"
      )}
      aria-label="Interactive repository dependency graph"
    >
      <ReactFlow<ExplorerFlowNode, ExplorerEdge>
        key={`${mode}:${display.presentation}:${scopePath}:${focus ?? "none"}:${direction}:${depth}:${expanded ? "expanded" : "inline"}`}
        nodes={layout.nodes}
        edges={layout.edges}
        nodeTypes={nodeTypes}
        edgeTypes={edgeTypes}
        onNodeClick={actions.onNodeClick}
        onNodeDoubleClick={actions.onNodeDoubleClick}
        onNodeMouseEnter={actions.onNodeMouseEnter}
        onNodeMouseLeave={actions.onNodeMouseLeave}
        onEdgeClick={actions.onEdgeClick}
        onPaneClick={() => actions.setSelection(null)}
        nodesDraggable={false}
        nodesConnectable={false}
        zoomOnDoubleClick={false}
        panOnScroll
        minZoom={0.08}
        maxZoom={1.5}
        fitView
        fitViewOptions={{
          padding: 0.16,
          minZoom: initialFitMinZoom,
          maxZoom: 1,
        }}
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
        <GraphMiniMap model={model} />
      </ReactFlow>
    </div>
  )
}

function GraphMiniMap({ model }: { model: GraphExplorerViewModel }) {
  if (model.display.entities.length <= 8) return null
  return (
    <MiniMap
      pannable
      zoomable
      ariaLabel="Repository graph overview"
      style={{
        width: model.expanded ? 260 : 224,
        height: model.expanded ? 176 : 152,
      }}
      bgColor="#0b0f14"
      maskColor="rgba(2, 6, 23, 0.66)"
      maskStrokeColor="#94a3b8"
      maskStrokeWidth={1.5}
      nodeBorderRadius={4}
      nodeStrokeColor="#020617"
      nodeStrokeWidth={8}
      nodeColor={(node) => miniMapNodeColor(node.data)}
    />
  )
}
