import { useState } from "react"
import {
  AlertTriangle,
  ArrowDownToLine,
  ArrowUpFromLine,
  Check,
  ChevronRight,
  Copy,
  FileCode2,
  Gauge,
  GitBranch,
  Layers3,
  Network,
} from "lucide-react"

import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs"
import {
  type ExplorerConnection,
  type ExplorerEntity,
  type ExplorerFileInspection,
  type ExplorerNeighborhoodPresentation,
  type ExplorerView,
} from "@/lib/graph-explorer-model"
import type { GraphDirection } from "@/lib/graph-data"
import { formatCompact, formatNumber, formatScore } from "@/lib/format"
import type { GraphEdge } from "@/lib/types"
import {
  categoryLabel,
  graphEdgeId,
  relationLabel,
  resolverDescription,
  resolverLabel,
  shortDate,
} from "@/components/repository-graph-visuals"
import { cn } from "@/lib/utils"

type ExploreFile = (
  path: string,
  direction?: GraphDirection,
  presentation?: ExplorerNeighborhoodPresentation
) => void

export function GraphFileDetails({
  inspection,
  className,
  onLocate,
  onExplore,
}: {
  inspection: ExplorerFileInspection
  className: string
  onLocate: (path: string) => void
  onExplore: ExploreFile
}) {
  return (
    <aside className={cn(className, "p-4")}>
      <Tabs key={inspection.file.path} defaultValue="info">
        <TabsList className="grid w-full grid-cols-2">
          <TabsTrigger value="info">Info</TabsTrigger>
          <TabsTrigger value="relations">Relations</TabsTrigger>
        </TabsList>
        <TabsContent value="info" className="pt-3">
          <FileInfo inspection={inspection} onExplore={onExplore} />
        </TabsContent>
        <TabsContent value="relations" className="pt-3">
          <FileRelations inspection={inspection} onLocate={onLocate} />
        </TabsContent>
      </Tabs>
    </aside>
  )
}

function FileInfo({
  inspection,
  onExplore,
}: {
  inspection: ExplorerFileInspection
  onExplore: ExploreFile
}) {
  const { file, findings, risk } = inspection
  const report = file.report
  const functions = [...(report.complexity?.functions ?? [])].sort(
    (left, right) =>
      right.cyclomatic - left.cyclomatic || left.line - right.line
  )
  const markers = Object.values(report.markers ?? {}).reduce(
    (total, count) => total + count,
    0
  )
  return (
    <>
      <div className="flex items-start justify-between gap-3">
        <div className="flex flex-wrap items-center gap-2">
          <Badge>{categoryLabel(file.category)}</Badge>
          <Badge variant="secondary">{report.language}</Badge>
          {risk ? (
            <Badge variant={risk.score >= 0.7 ? "destructive" : "outline"}>
              Risk {formatScore(risk.score)}
            </Badge>
          ) : null}
        </div>
        <CopyPathButton path={file.path} />
      </div>
      <h3 className="mt-3 break-all font-mono text-sm font-semibold leading-relaxed">
        {file.path}
      </h3>
      <GraphActions inspection={inspection} onExplore={onExplore} />
      <LatestScan
        inspection={inspection}
        callableCount={functions.length}
        markerCount={markers}
      />
      <SymbolSummary inspection={inspection} />
      <CallableList functions={functions} />
      <MarkerList inspection={inspection} />
      {findings.length > 0 ? <FileFindings findings={findings} /> : null}
    </>
  )
}

function GraphActions({
  inspection,
  onExplore,
}: {
  inspection: ExplorerFileInspection
  onExplore: ExploreFile
}) {
  const { file, graph } = inspection
  if (!graph) return null
  return (
    <>
      <div className="mt-4 grid grid-cols-2 gap-2">
        {graph.symbolRelations.length > 0 ? (
          <Button
            className="col-span-2"
            size="sm"
            onClick={() => onExplore(file.path, "both", "type")}
          >
            <GitBranch /> Type structure
          </Button>
        ) : null}
        <Button
          size="sm"
          variant="outline"
          onClick={() => onExplore(file.path, "dependencies", "full")}
        >
          <ArrowDownToLine /> Dependencies
        </Button>
        <Button
          size="sm"
          variant="outline"
          onClick={() => onExplore(file.path, "dependents", "full")}
        >
          <ArrowUpFromLine /> Blast radius
        </Button>
        <Button
          className="col-span-2"
          size="sm"
          variant="secondary"
          onClick={() => onExplore(file.path, "both", "full")}
        >
          <Layers3 /> Full neighborhood
        </Button>
      </div>
      <section className="mt-5 rounded-lg border bg-muted/30 p-3">
        <div className="flex items-center justify-between gap-2">
          <h4 className="flex items-center gap-2 text-xs font-semibold uppercase tracking-wide text-muted-foreground">
            <Network className="size-3.5" /> Structural reach
          </h4>
          <Badge
            variant={graph.prominence.level === "hub" ? "default" : "outline"}
          >
            {graph.prominence.label}
          </Badge>
        </div>
        <p className="mt-2 text-xs leading-relaxed">
          {graph.prominence.reason}
        </p>
        <p className="mt-1.5 text-[10px] leading-relaxed text-muted-foreground">
          {graph.prominence.basis === "symbol"
            ? "Node size uses explicit extends, implements, or embeds syntax resolved to an unambiguous repository symbol."
            : "Node size uses resolved file dependencies; ambiguous type relationships are never inferred."}
        </p>
      </section>
    </>
  )
}

function LatestScan({
  inspection,
  callableCount,
  markerCount,
}: {
  inspection: ExplorerFileInspection
  callableCount: number
  markerCount: number
}) {
  const { file, findings, graph } = inspection
  const report = file.report
  return (
    <section className="mt-5 border-t pt-4">
      <h4 className="flex items-center gap-2 text-xs font-semibold uppercase tracking-wide text-muted-foreground">
        <Gauge className="size-3.5" /> Latest scan
      </h4>
      <div className="mt-2 grid grid-cols-2 gap-2">
        <DetailMetric
          label="Tokens"
          value={formatCompact(report.tokens)}
          detail={`${formatNumber(report.sloc)} SLOC`}
        />
        <DetailMetric
          label="Lines"
          value={formatCompact(report.loc)}
          detail={`${Math.round(report.comment_ratio * 100)}% comments`}
        />
        <DetailMetric
          label="Fan in / out"
          value={graph ? `${graph.file.fan_in} / ${graph.file.fan_out}` : "—"}
          detail={graph ? "direct links" : "not in topology"}
        />
        <DetailMetric
          label="Cyclomatic"
          value={
            report.complexity ? formatNumber(report.complexity.cyclomatic) : "—"
          }
          detail={
            report.complexity
              ? `${formatNumber(report.complexity.cognitive)} cognitive`
              : "not measured"
          }
        />
        <DetailMetric
          label="Maintainability"
          value={
            report.complexity
              ? report.complexity.maintainability_index.toFixed(1)
              : "—"
          }
          detail={report.approximate ? "approximate" : "index"}
        />
        <DetailMetric
          label="Max nesting"
          value={
            report.complexity
              ? formatNumber(report.complexity.max_nesting)
              : "—"
          }
          detail={
            callableCount > 0
              ? `${formatNumber(callableCount)} callables`
              : "no callables"
          }
        />
        <DetailMetric
          label="Churn"
          value={report.churn ? formatNumber(report.churn.commits) : "—"}
          detail={
            report.churn
              ? `${formatNumber(report.churn.authors)} authors`
              : "not measured"
          }
        />
        <DetailMetric
          label="Signals"
          value={formatNumber(findings.length)}
          detail={`${formatNumber(markerCount)} markers`}
        />
      </div>
      {report.churn?.first_commit || report.churn?.last_commit ? (
        <p className="mt-2 rounded-lg bg-muted/60 px-3 py-2 text-[10px] text-muted-foreground">
          History {shortDate(report.churn.first_commit)} →{" "}
          {shortDate(report.churn.last_commit)}
        </p>
      ) : null}
    </section>
  )
}

function SymbolSummary({ inspection }: { inspection: ExplorerFileInspection }) {
  const symbols = inspection.file.report.symbols
  if (!symbols) return null
  return (
    <section className="mt-5 border-t pt-4">
      <h4 className="flex items-center gap-2 text-xs font-semibold uppercase tracking-wide text-muted-foreground">
        <FileCode2 className="size-3.5" /> Symbols
      </h4>
      <div className="mt-2 grid grid-cols-3 gap-2">
        <MiniMetric label="Functions" value={symbols.functions} />
        <MiniMetric label="Types" value={symbols.types} />
        <MiniMetric label="Exports" value={symbols.exports} />
      </div>
    </section>
  )
}

function CallableList({
  functions,
}: {
  functions: NonNullable<
    NonNullable<
      ExplorerFileInspection["file"]["report"]["complexity"]
    >["functions"]
  >
}) {
  if (functions.length === 0) return null
  return (
    <section className="mt-5 border-t pt-4">
      <h4 className="text-xs font-semibold uppercase tracking-wide text-muted-foreground">
        Callables{" "}
        <span className="float-right tabular-nums">{functions.length}</span>
      </h4>
      <div className="mt-2 space-y-1.5">
        {functions.slice(0, 20).map((fn) => (
          <div
            key={`${fn.symbol_key ?? fn.name}:${fn.line}`}
            className="rounded-lg border px-3 py-2"
          >
            <div className="flex items-center justify-between gap-2">
              <span
                className="min-w-0 truncate font-mono text-[11px] font-medium"
                title={fn.name}
              >
                {fn.name}
              </span>
              <Badge variant={fn.cyclomatic > 20 ? "destructive" : "outline"}>
                C{fn.cyclomatic}
              </Badge>
            </div>
            <p className="mt-1 text-[10px] text-muted-foreground">
              line {fn.line}
              {fn.end_line ? `–${fn.end_line}` : ""} · cognitive {fn.cognitive}{" "}
              · nesting {fn.max_nesting}
            </p>
          </div>
        ))}
      </div>
    </section>
  )
}

function MarkerList({ inspection }: { inspection: ExplorerFileInspection }) {
  const occurrences = inspection.file.report.marker_occurrences ?? []
  if (occurrences.length === 0) return null
  return (
    <section className="mt-5 border-t pt-4">
      <h4 className="text-xs font-semibold uppercase tracking-wide text-muted-foreground">
        Markers
      </h4>
      <div className="mt-2 flex flex-wrap gap-1.5">
        {occurrences.slice(0, 30).map((marker) => (
          <Badge
            key={`${marker.marker}:${marker.line}:${marker.occurrence}`}
            variant="outline"
          >
            {marker.marker} · line {marker.line}
          </Badge>
        ))}
      </div>
    </section>
  )
}

function FileRelations({
  inspection,
  onLocate,
}: {
  inspection: ExplorerFileInspection
  onLocate: (path: string) => void
}) {
  const { graph } = inspection
  if (!graph) {
    return (
      <div className="rounded-lg border border-dashed p-4 text-sm text-muted-foreground">
        This recognized file contributes scope metrics but is not a first-class
        dependency node.
      </div>
    )
  }
  return (
    <>
      <div className="flex flex-wrap gap-1.5">
        {graph.roles.map((role) => (
          <Badge key={role} variant="outline">
            {role}
          </Badge>
        ))}
      </div>
      <ResolverUsage inspection={inspection} />
      <SymbolRelations inspection={inspection} onLocate={onLocate} />
      <ConnectionList
        title="Dependencies"
        icon={ArrowDownToLine}
        edges={graph.outgoing}
        pathKey="target"
        onLocate={onLocate}
      />
      <ConnectionList
        title="Dependents"
        icon={ArrowUpFromLine}
        edges={graph.incoming}
        pathKey="source"
        onLocate={onLocate}
      />
      <CycleMembership inspection={inspection} onLocate={onLocate} />
      <ImportedRoots inspection={inspection} />
    </>
  )
}

function ResolverUsage({ inspection }: { inspection: ExplorerFileInspection }) {
  const usages = inspection.graph?.resolverUsage ?? []
  if (usages.length === 0) return null
  return (
    <section className="mt-5 border-t pt-4">
      <h4 className="flex items-center gap-2 text-xs font-semibold uppercase tracking-wide text-muted-foreground">
        <Network className="size-3.5" /> Resolver provenance
      </h4>
      <div className="mt-2 flex flex-wrap gap-1.5">
        {usages.map((usage) => (
          <Badge key={usage.resolver} variant="secondary">
            {resolverLabel(usage.resolver)} · {usage.connections}
          </Badge>
        ))}
      </div>
    </section>
  )
}

function SymbolRelations({
  inspection,
  onLocate,
}: {
  inspection: ExplorerFileInspection
  onLocate: (path: string) => void
}) {
  const relations = inspection.graph?.symbolRelations ?? []
  if (relations.length === 0) return null
  return (
    <section className="mt-5 border-t pt-4">
      <h4 className="flex items-center gap-2 text-xs font-semibold uppercase tracking-wide text-muted-foreground">
        <GitBranch className="size-3.5" /> Explicit type relationships
      </h4>
      <div className="mt-2 space-y-1.5">
        {relations.slice(0, 30).map(({ direction, relation, symbol }) => (
          <button
            key={`${direction}:${relation}:${symbol.id}`}
            type="button"
            onClick={() => onLocate(symbol.path)}
            className="flex w-full items-center gap-2 rounded-lg border px-3 py-2 text-left hover:bg-accent"
          >
            <span className="text-muted-foreground">
              {direction === "incoming" ? "←" : "→"}
            </span>
            <Badge variant="outline">{relation}</Badge>
            <span className="min-w-0 flex-1">
              <span className="block truncate font-mono text-[11px] font-medium">
                {symbol.qualified_name}
              </span>
              <span className="block truncate text-[10px] text-muted-foreground">
                {symbol.path}:{symbol.line}
              </span>
            </span>
          </button>
        ))}
      </div>
    </section>
  )
}

function CycleMembership({
  inspection,
  onLocate,
}: {
  inspection: ExplorerFileInspection
  onLocate: (path: string) => void
}) {
  const cycles = inspection.graph?.cycles ?? []
  if (cycles.length === 0) return null
  return (
    <section className="mt-5 border-t pt-4">
      <h4 className="flex items-center gap-2 text-xs font-semibold uppercase tracking-wide text-muted-foreground">
        <GitBranch className="size-3.5" /> Cycle membership
      </h4>
      <div className="mt-2 space-y-2">
        {cycles.map((cycle, index) => (
          <div
            key={cycle.join("→")}
            className="rounded-lg border border-destructive/25 bg-destructive/5 p-2.5"
          >
            <p className="text-[10px] font-semibold uppercase tracking-wide text-destructive">
              Cycle {index + 1} · {cycle.length} files
            </p>
            {cycle.map((path) => (
              <button
                key={path}
                type="button"
                onClick={() => onLocate(path)}
                className="mt-1 block w-full truncate rounded px-1 py-1 text-left font-mono text-[10px] hover:bg-accent"
              >
                {path}
              </button>
            ))}
          </div>
        ))}
      </div>
    </section>
  )
}

function ImportedRoots({ inspection }: { inspection: ExplorerFileInspection }) {
  const imports = inspection.file.report.imports ?? []
  if (imports.length === 0) return null
  return (
    <section className="mt-5 border-t pt-4">
      <h4 className="text-xs font-semibold uppercase tracking-wide text-muted-foreground">
        Imported roots
      </h4>
      <div className="mt-2 flex flex-wrap gap-1.5">
        {imports.map((dependency) => (
          <Badge key={dependency} variant="outline">
            {dependency}
          </Badge>
        ))}
      </div>
    </section>
  )
}

export function GraphConnectionDetails({
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
        <Badge variant="outline">
          {connection.count} file connection{connection.count === 1 ? "" : "s"}
        </Badge>
      </div>
      <h3 className="mt-3 text-sm font-semibold">Connection details</h3>

      <div className="mt-4 grid gap-2">
        <EntityEndpoint label="Source" entity={source} />
        <ArrowDownToLine className="mx-auto size-4 text-muted-foreground" />
        <EntityEndpoint label="Target" entity={target} />
      </div>

      <section className="mt-5 border-t pt-4">
        <h4 className="text-xs font-semibold uppercase tracking-wide text-muted-foreground">
          Resolver provenance
        </h4>
        <div className="mt-2 space-y-2">
          {connection.resolvers.map((usage) => (
            <div
              key={usage.resolver}
              className="rounded-lg bg-muted/60 px-3 py-2"
            >
              <div className="flex items-center justify-between gap-2 text-xs font-medium">
                <span>{resolverLabel(usage.resolver)}</span>
                <span className="tabular-nums text-muted-foreground">
                  {usage.connections}
                </span>
              </div>
              <p className="mt-1 text-[10px] leading-relaxed text-muted-foreground">
                {resolverDescription(usage.resolver)}
              </p>
            </div>
          ))}
        </div>
      </section>

      <section className="mt-5 border-t pt-4">
        <h4 className="text-xs font-semibold uppercase tracking-wide text-muted-foreground">
          File connections{" "}
          <span className="float-right tabular-nums">
            {connection.fileEdges.length}
          </span>
        </h4>
        <div className="mt-2 space-y-1.5">
          {connection.fileEdges.slice(0, 50).map((edge) => (
            <div
              key={graphEdgeId(edge)}
              className="rounded-lg border bg-background p-2.5"
            >
              <button
                type="button"
                onClick={() => onLocate(edge.source)}
                className="block w-full truncate text-left font-mono text-[10px] hover:underline"
                title={edge.source}
              >
                {edge.source}
              </button>
              <div className="my-1 flex items-center gap-1 text-[9px] text-muted-foreground">
                <ChevronRight className="size-3" />{" "}
                {resolverLabel(edge.resolver)}
              </div>
              <button
                type="button"
                onClick={() => onLocate(edge.target)}
                className="block w-full truncate text-left font-mono text-[10px] hover:underline"
                title={edge.target}
              >
                {edge.target}
              </button>
            </div>
          ))}
        </div>
      </section>
    </aside>
  )
}

function EntityEndpoint({
  label,
  entity,
}: {
  label: string
  entity?: ExplorerEntity
}) {
  if (!entity) return null
  return (
    <div className="rounded-lg border bg-background p-3">
      <span className="text-[10px] font-semibold uppercase tracking-wide text-muted-foreground">
        {label}
      </span>
      <span className="mt-1 block break-all font-mono text-xs font-medium">
        {entity.path || "Project"}
      </span>
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
        <Icon className="size-3.5" /> {title}{" "}
        <span className="ml-auto tabular-nums">{edges.length}</span>
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
                <span
                  className="block truncate font-mono text-[11px]"
                  title={path}
                >
                  {path}
                </span>
                <span className="mt-0.5 block text-[10px] text-muted-foreground">
                  {resolverLabel(edge.resolver)}
                </span>
              </button>
            )
          })}
        </div>
      )}
    </section>
  )
}

function FileFindings({
  findings,
}: {
  findings: ExplorerFileInspection["findings"]
}) {
  return (
    <section className="mt-5 border-t pt-4">
      <h4 className="flex items-center gap-2 text-xs font-semibold uppercase tracking-wide text-muted-foreground">
        <AlertTriangle className="size-3.5" /> Findings{" "}
        <span className="ml-auto tabular-nums">{findings.length}</span>
      </h4>
      <div className="mt-2 space-y-2">
        {findings.slice(0, 20).map((finding) => (
          <div
            key={`${finding.fingerprint}:${finding.primary_location.start_line}`}
            className="rounded-lg border p-2.5"
          >
            <div className="flex flex-wrap items-center gap-1.5">
              <Badge
                variant={
                  finding.severity === "error" ? "destructive" : "secondary"
                }
              >
                {finding.severity}
              </Badge>
              <span className="text-[10px] text-muted-foreground">
                {finding.kind} · line {finding.primary_location.start_line}
              </span>
            </div>
            <p className="mt-1.5 text-xs leading-relaxed">{finding.message}</p>
            {finding.metrics && Object.keys(finding.metrics).length > 0 ? (
              <p className="mt-1 font-mono text-[9px] text-muted-foreground">
                {Object.entries(finding.metrics)
                  .map(([name, value]) => `${name}=${value}`)
                  .join(" · ")}
              </p>
            ) : null}
          </div>
        ))}
      </div>
    </section>
  )
}

export function DetailMetric({
  label,
  value,
  detail,
}: {
  label: string
  value: string
  detail: string
}) {
  return (
    <div className="rounded-lg bg-muted/60 px-3 py-2">
      <div className="text-[10px] uppercase tracking-wide text-muted-foreground">
        {label}
      </div>
      <div className="mt-0.5 text-base font-semibold tabular-nums">{value}</div>
      <div className="truncate text-[10px] text-muted-foreground">{detail}</div>
    </div>
  )
}

function MiniMetric({ label, value }: { label: string; value: number }) {
  return (
    <div className="rounded-lg bg-muted/60 px-2 py-2 text-center">
      <div className="text-base font-semibold tabular-nums">
        {formatNumber(value)}
      </div>
      <div className="text-[9px] uppercase tracking-wide text-muted-foreground">
        {label}
      </div>
    </div>
  )
}

function CopyPathButton({ path }: { path: string }) {
  const [copiedPath, setCopiedPath] = useState<string | null>(null)
  const copied = copiedPath === path
  const copy = async () => {
    try {
      await navigator.clipboard.writeText(path)
      setCopiedPath(path)
    } catch {
      setCopiedPath(null)
    }
  }
  return (
    <Button
      type="button"
      size="icon-sm"
      variant="ghost"
      aria-label={copied ? "Path copied" : "Copy file path"}
      onClick={() => void copy()}
    >
      {copied ? <Check /> : <Copy />}
    </Button>
  )
}
