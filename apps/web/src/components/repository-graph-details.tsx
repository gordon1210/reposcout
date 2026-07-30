import { Boxes, ChevronRight, Gauge } from "lucide-react"

import {
  GraphConnectionDetails,
  GraphFileDetails,
  DetailMetric,
} from "@/components/repository-graph-file-details"
import type { GraphSelection } from "@/components/repository-graph-types"
import { Badge } from "@/components/ui/badge"
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs"
import {
  type ExplorerConnection,
  type ExplorerGroup,
  type ExplorerNeighborhoodPresentation,
  type ExplorerScopeInspection,
  type ExplorerView,
  type RepositoryGraphExplorer,
} from "@/lib/graph-explorer-model"
import type { GraphDirection } from "@/lib/graph-data"
import { formatCompact, formatNumber } from "@/lib/format"
import {
  categoryLabel,
  languageColor,
  scopeColor,
  scopeKindLabel,
} from "@/components/repository-graph-visuals"
import { cn } from "@/lib/utils"

export function GraphDetails({
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
    presentation?: ExplorerNeighborhoodPresentation
  ) => void
}) {
  const className = cn(
    "h-[52rem] overflow-auto rounded-xl border bg-card",
    expanded && "h-auto min-h-80 lg:h-full"
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
    const group = view.groups?.find(
      (candidate) => candidate.id === selection.id
    )
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
    presentation?: ExplorerNeighborhoodPresentation
  ) => void
}) {
  const relationship = group.relationship!
  const totalMembers = group.totalMembers ?? group.members.length
  const memberCount =
    group.members.length < totalMembers
      ? `${group.members.length} of ${totalMembers}`
      : String(group.members.length)
  return (
    <aside className={cn(className, "p-4")}>
      <div className="flex flex-wrap items-center gap-2">
        <Badge>
          {relationship.family === "type"
            ? "Explicit type relationship"
            : "Direct import context"}
        </Badge>
        <Badge variant="outline">{relationship.direction}</Badge>
        <Badge variant="secondary">{memberCount} files</Badge>
      </div>
      <h3 className="mt-3 text-base font-semibold">
        {group.label} {group.name}
      </h3>
      <p className="mt-2 text-xs leading-relaxed text-muted-foreground">
        {relationship.description}
      </p>
      <section className="mt-5 border-t pt-4">
        <h4 className="text-xs font-semibold uppercase tracking-wide text-muted-foreground">
          Members{" "}
          <span className="float-right tabular-nums">{memberCount}</span>
        </h4>
        <div className="mt-2 space-y-1.5">
          {group.members.map((member) => (
            <button
              key={member.id}
              type="button"
              onClick={() =>
                member.kind === "file" && onExplore(member.path, "both", "auto")
              }
              className="flex w-full items-center gap-2 rounded-lg border bg-background px-3 py-2 text-left hover:bg-accent focus-visible:bg-accent focus-visible:outline-none"
            >
              <span
                className="size-2 shrink-0 rounded-full"
                style={{
                  background:
                    member.kind === "file"
                      ? languageColor(member.report.language)
                      : scopeColor(member),
                }}
              />
              <span className="min-w-0 flex-1">
                <span className="block truncate font-mono text-[11px] font-medium">
                  {member.path}
                </span>
                <span className="mt-0.5 block text-[10px] text-muted-foreground">
                  {member.kind === "file"
                    ? `${member.report.language} · ${formatCompact(member.report.tokens)} tokens`
                    : `${member.files} files`}
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
  const markers = [...markerTotals.values()].reduce(
    (total, count) => total + count,
    0
  )
  const topMarker = [...markerTotals.entries()]
    .sort(
      (left, right) => right[1] - left[1] || left[0].localeCompare(right[0])
    )
    .at(0)
  const churnCommits = scanned.reduce(
    (total, file) => total + (file.report.churn?.commits ?? 0),
    0
  )
  const filesWithHistory = scanned.filter(
    (file) => (file.report.churn?.commits ?? 0) > 0
  ).length
  const loc = scanned.reduce((total, file) => total + file.report.loc, 0)
  const commentLines = scanned.reduce(
    (total, file) => total + file.report.comment_lines,
    0
  )
  const commentPct = loc > 0 ? Math.round((commentLines / loc) * 100) : 0

  return (
    <aside className={cn(className, "p-4")}>
      <Tabs key={inspection.path || "."} defaultValue="info">
        <TabsList className="grid w-full grid-cols-2">
          <TabsTrigger value="info">Info</TabsTrigger>
          <TabsTrigger value="files">
            Files <span className="tabular-nums">{inspection.files}</span>
          </TabsTrigger>
        </TabsList>
        <TabsContent value="info" className="pt-3">
          <div className="flex flex-wrap items-center gap-2">
            <Badge>{scopeKindLabel(inspection.scopeKind)}</Badge>
            {inspection.riskFiles > 0 ? (
              <Badge variant="destructive">
                {inspection.riskFiles} risk files
              </Badge>
            ) : null}
            {inspection.findings > 0 ? (
              <Badge variant="secondary">{inspection.findings} findings</Badge>
            ) : null}
          </div>
          <h3 className="mt-3 break-all text-base font-semibold">
            {inspection.name}
          </h3>
          <p className="mt-1 break-all font-mono text-xs text-muted-foreground">
            {inspection.path || "."}
          </p>

          <section className="mt-5">
            <h4 className="flex items-center gap-2 text-xs font-semibold uppercase tracking-wide text-muted-foreground">
              <Gauge className="size-3.5" /> Scope facts
            </h4>
            <div className="mt-2 grid grid-cols-2 gap-2">
              <DetailMetric
                label="Files"
                value={formatNumber(inspection.files)}
                detail={`${inspection.graphFiles} in topology`}
              />
              <DetailMetric
                label="Tokens"
                value={formatCompact(inspection.tokens)}
                detail={`${formatNumber(inspection.sloc)} SLOC`}
              />
              <DetailMetric
                label="Lines"
                value={formatCompact(loc)}
                detail={`${commentPct}% comments`}
              />
              <DetailMetric
                label="Test files"
                value={formatNumber(testFiles)}
                detail={`${formatNumber(scanned.length - testFiles)} non-test files`}
              />
              <DetailMetric
                label="Fan in / out"
                value={`${inspection.fanIn} / ${inspection.fanOut}`}
                detail="cross-scope edges"
              />
              <DetailMetric
                label="Max cyclomatic"
                value={formatNumber(inspection.maxCyclomatic)}
                detail="highest file total"
              />
              <DetailMetric
                label="Min maintainability"
                value={inspection.minMaintainability?.toFixed(1) ?? "—"}
                detail="lowest file index"
              />
              <DetailMetric
                label="Churn"
                value={formatNumber(churnCommits)}
                detail={
                  filesWithHistory > 0
                    ? `commits · ${formatNumber(filesWithHistory)} files with history`
                    : "no git history"
                }
              />
              <DetailMetric
                label="Markers"
                value={formatNumber(markers)}
                detail={
                  topMarker
                    ? `mostly ${topMarker[0]} ×${topMarker[1]}`
                    : "no TODO-style markers"
                }
              />
              <DetailMetric
                label="Signals"
                value={formatNumber(inspection.findings)}
                detail={`${inspection.riskFiles} ranked risks`}
              />
            </div>
          </section>

          <section className="mt-5 border-t pt-4">
            <h4 className="flex items-center gap-2 text-xs font-semibold uppercase tracking-wide text-muted-foreground">
              <Boxes className="size-3.5" /> Languages
            </h4>
            <div className="mt-2 flex flex-wrap gap-1.5">
              {inspection.languages.map((language) => (
                <Badge key={language.name} variant="outline">
                  <span
                    className="mr-1.5 size-1.5 rounded-full"
                    style={{ background: languageColor(language.name) }}
                  />
                  {language.name} · {language.files}
                </Badge>
              ))}
            </div>
          </section>

          <section className="mt-5 border-t pt-4">
            <h4 className="text-xs font-semibold uppercase tracking-wide text-muted-foreground">
              Topology coverage
            </h4>
            <p className="mt-2 rounded-lg bg-muted/60 px-3 py-2 text-xs leading-relaxed text-muted-foreground">
              {inspection.graphFiles} of {inspection.files} scanned files are
              first-class topology nodes. Other recognized files remain visible
              in the Files tab and scope totals without invented relationships.
            </p>
            {inspection.configFiles.length > 0 ? (
              <div className="mt-2 flex flex-wrap gap-1.5">
                {inspection.configFiles.map((path) => (
                  <Badge key={path} variant="secondary">
                    {path}
                  </Badge>
                ))}
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
                <span
                  className="size-2 shrink-0 rounded-full"
                  style={{ background: languageColor(file.report.language) }}
                />
                <span className="min-w-0 flex-1">
                  <span className="block truncate font-mono text-[11px]">
                    {file.path}
                  </span>
                  <span className="mt-0.5 block text-[10px] text-muted-foreground">
                    {categoryLabel(file.category)} · {file.report.language} ·{" "}
                    {formatCompact(file.report.tokens)} tokens
                  </span>
                </span>
              </button>
            ))}
            {inspection.allFiles.length > 150 ? (
              <p className="px-2 py-2 text-[10px] text-muted-foreground">
                Showing 150 of {inspection.allFiles.length} files. Use search to
                locate another file.
              </p>
            ) : null}
          </div>
        </TabsContent>
      </Tabs>
    </aside>
  )
}
