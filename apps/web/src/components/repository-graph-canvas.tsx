import { memo } from "react"
import {
  BaseEdge,
  EdgeLabelRenderer,
  Handle,
  Position,
  getBezierPath,
  useStore,
  type EdgeProps,
  type NodeProps,
} from "@xyflow/react"

import type {
  ExplorerEntity,
  ExplorerFileSummary,
} from "@/lib/graph-explorer-model"
import type { GraphProminence } from "@/lib/graph-data"
import { formatCompact } from "@/lib/format"
import {
  categoryLabel,
  languageColor,
  relationLabel,
  scopeColor,
  scopeKindLabel,
} from "@/components/repository-graph-visuals"
import type {
  ExplorerEdge,
  ExplorerGroupFlowNode,
  ExplorerNode,
  ExplorerNodeData,
} from "@/components/repository-graph-types"
import { cn } from "@/lib/utils"

const GLANCE_MAX_ZOOM = 0.5

const glanceZoomSelector = (state: {
  transform: [number, number, number]
}): boolean => state.transform[2] < GLANCE_MAX_ZOOM

export const ExplorerEntityNode = memo(function ExplorerEntityNode({
  data,
  selected,
}: NodeProps<ExplorerNode>) {
  const entity = data.entity
  const glance = useStore(glanceZoomSelector)
  const handleColor =
    entity.kind === "file"
      ? languageColor(entity.report.language)
      : scopeColor(entity)
  return (
    <div
      style={{ width: data.width, height: data.height }}
      className={entityNodeClassName(data, selected)}
    >
      <span
        className="absolute inset-y-2 left-0 w-1 rounded-r-full"
        style={{ background: handleColor }}
      />
      <Handle
        type="target"
        position={data.vertical ? Position.Top : Position.Left}
        className="!size-2 !border-background"
        style={{ background: handleColor }}
      />
      <EntityNodeBody
        entity={entity}
        prominence={data.prominence}
        glance={glance}
      />
      <Handle
        type="source"
        position={data.vertical ? Position.Bottom : Position.Right}
        className="!size-2 !border-background"
        style={{ background: handleColor }}
      />
    </div>
  )
})

function entityNodeClassName(
  data: ExplorerNodeData,
  selected: boolean
): string {
  const entity = data.entity
  return cn(
    "relative h-full cursor-pointer rounded-xl border bg-card/95 text-card-foreground shadow-sm backdrop-blur transition-[border-color,box-shadow,opacity,filter]",
    entityNodePadding(entity, data.prominence),
    entity.external && "border-dashed bg-card/75",
    entity.kind === "file" &&
      data.prominence.level === "hub" &&
      "border-primary/55 shadow-lg ring-1 ring-primary/10",
    entity.kind === "file" &&
      data.prominence.level === "notable" &&
      "border-foreground/30 shadow-md",
    data.typeFocused && "border-primary shadow-xl ring-2 ring-primary/25",
    data.focused && "border-foreground shadow-md ring-2 ring-foreground/10",
    data.related && !selected && "border-foreground/45 shadow-sm",
    selected && "border-ring shadow-md ring-2 ring-ring/25",
    !selected && "hover:border-foreground/60",
    data.dimmed && "opacity-25 saturate-50"
  )
}

function entityNodePadding(
  entity: ExplorerEntity,
  prominence: GraphProminence
): string {
  if (entity.kind === "scope" || prominence.level === "hub") {
    return "px-4 py-3"
  }
  return prominence.level === "notable" ? "px-3.5 py-3" : "px-3 py-2.5"
}

function EntityNodeBody({
  entity,
  prominence,
  glance,
}: {
  entity: ExplorerEntity
  prominence: GraphProminence
  glance: boolean
}) {
  if (entity.kind === "scope") {
    return glance ? (
      <ScopeNodeGlance scope={entity} />
    ) : (
      <ScopeNodeContent scope={entity} />
    )
  }
  return glance ? (
    <FileNodeGlance file={entity} />
  ) : (
    <FileNodeContent file={entity} prominence={prominence} />
  )
}

function ScopeNodeContent({
  scope,
}: {
  scope: Extract<ExplorerEntity, { kind: "scope" }>
}) {
  return (
    <div className="min-w-0 pl-1">
      <div className="flex items-center justify-between gap-3 text-[10px] font-semibold uppercase tracking-wide">
        <span style={{ color: scopeColor(scope) }}>
          {scope.external ? "Boundary" : scopeKindLabel(scope.scopeKind)}
        </span>
        <span className="text-muted-foreground">
          {scope.findings > 0
            ? `${scope.findings} signals`
            : `${scope.graphFiles} connected`}
        </span>
      </div>
      <div
        className="mt-2 truncate text-sm font-semibold"
        title={scope.path || "Project"}
      >
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
          <span
            key={language.name}
            className="truncate rounded-full bg-muted px-1.5 py-0.5 text-[9px] text-muted-foreground"
          >
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

function FileNodeContent({
  file,
  prominence,
}: {
  file: ExplorerFileSummary
  prominence: GraphProminence
}) {
  const graph = file.graphFile
  const metric = file.report.complexity
    ? `C${file.report.complexity.cyclomatic}`
    : file.report.language
  return (
    <div className="min-w-0 pl-1">
      <div className="flex items-center justify-between gap-3 text-[10px] font-semibold uppercase tracking-wide">
        <span style={{ color: languageColor(file.report.language) }}>
          {categoryLabel(file.category)}
        </span>
        <span
          className={cn(
            "truncate text-muted-foreground",
            prominence.level === "hub" && "text-primary",
            prominence.level === "notable" && "text-foreground/75"
          )}
          title={prominence.reason}
        >
          {prominenceLabel(prominence, metric)}
        </span>
      </div>
      <div
        className="mt-1.5 truncate font-mono text-xs font-semibold"
        title={file.path}
      >
        {file.name}
      </div>
      <div className="mt-0.5 truncate font-mono text-[10px] text-muted-foreground">
        {file.parentPath || "."}
      </div>
      <div className="mt-2 flex gap-3 text-[10px] text-muted-foreground tabular-nums">
        <span>{formatCompact(file.report.tokens)} tok</span>
        <span>
          {graph ? `${graph.fan_in} in · ${graph.fan_out} out` : "metrics only"}
        </span>
      </div>
    </div>
  )
}

function prominenceLabel(
  prominence: GraphProminence,
  standardMetric: string
): string {
  if (prominence.level === "standard") return standardMetric
  if (prominence.basis === "symbol") {
    return `${prominence.label} · ${prominence.reach}`
  }
  return prominence.label
}

function ScopeNodeGlance({
  scope,
}: {
  scope: Extract<ExplorerEntity, { kind: "scope" }>
}) {
  return (
    <div className="flex h-full min-w-0 flex-col items-center justify-center gap-1 px-1 text-center">
      <span
        className="w-full truncate text-2xl font-bold leading-tight"
        title={scope.path || "Project"}
      >
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
      <span
        className="w-full truncate font-mono text-xl font-bold leading-tight"
        title={file.path}
      >
        {file.name}
      </span>
      <span className="w-full truncate text-sm text-muted-foreground tabular-nums">
        {graph
          ? `${graph.fan_in} in · ${graph.fan_out} out`
          : file.report.language}
      </span>
    </div>
  )
}

export const ExplorerGroupNode = memo(function ExplorerGroupNode({
  data,
  selected,
}: NodeProps<ExplorerGroupFlowNode>) {
  const group = data.group
  const glance = useStore(glanceZoomSelector)
  const visibleMembers = group.members.length
  const totalMembers = group.totalMembers ?? visibleMembers
  const memberCount =
    visibleMembers < totalMembers
      ? `${visibleMembers} of ${totalMembers}`
      : String(visibleMembers)
  return (
    <div className={groupNodeClassName(data, selected)}>
      <div className={groupHeaderClassName(data, glance)}>
        <span
          className="min-w-0 truncate font-semibold"
          title={group.path || "."}
        >
          {group.label} · {group.name}
        </span>
        <span className="shrink-0 tabular-nums">
          {memberCount} {group.kind === "architecture" ? "items" : "files"}
        </span>
      </div>
    </div>
  )
})

function groupNodeClassName(
  data: ExplorerGroupFlowNode["data"],
  selected: boolean
): string {
  const group = data.group
  return cn(
    "h-full w-full cursor-pointer rounded-2xl border transition-[border-color,background-color,opacity]",
    group.kind === "architecture"
      ? "border-border/80 bg-card/45 shadow-md"
      : group.kind === "relationship" && group.relationship?.family === "type"
        ? "border-primary/55 bg-primary/5 shadow-md"
        : "border-dashed bg-muted/15 shadow-inner",
    selected && "border-ring bg-ring/5 ring-2 ring-ring/20",
    data.related && !selected && "border-foreground/40 bg-muted/25",
    data.dimmed && "opacity-20"
  )
}

function groupHeaderClassName(
  data: ExplorerGroupFlowNode["data"],
  glance: boolean
): string {
  const group = data.group
  return cn(
    "flex h-12 items-center justify-between gap-3 border-b px-4 text-muted-foreground",
    glance ? "text-lg" : "text-[10px] uppercase tracking-wide",
    group.kind === "architecture"
      ? "bg-muted/30"
      : group.kind === "relationship" && group.relationship?.family === "type"
        ? "border-primary/25 bg-primary/10 text-primary"
        : "border-dashed"
  )
}

export const ExplorerConnectionEdge = memo(function ExplorerConnectionEdge({
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
      <BaseEdge
        id={id}
        path={path}
        markerEnd={markerEnd}
        interactionWidth={28}
        style={style}
      />
      {data?.showLabel ? (
        <EdgeLabelRenderer>
          <div
            className="repository-graph-edge-label nodrag nopan pointer-events-none absolute whitespace-nowrap rounded-full border bg-popover/95 px-2 py-0.5 text-[9px] font-medium text-popover-foreground shadow-sm"
            style={{
              transform: `translate(-50%, -50%) translate(${labelX}px, ${labelY}px)`,
            }}
          >
            {relationLabel(data.connection.relation)}
            {data.connection.count > 1 ? ` · ${data.connection.count}` : ""}
          </div>
        </EdgeLabelRenderer>
      ) : null}
    </>
  )
})
