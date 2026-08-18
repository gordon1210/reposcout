import { lazy, Suspense, useMemo } from "react"
import {
  Activity,
  AlertTriangle,
  CheckCircle2,
  CircleDot,
  FileCode2,
  Gauge,
  Network,
  TestTube2,
} from "lucide-react"

import { Badge } from "@/components/ui/badge"
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card"
import { DataTable, DataTableColumnHeader } from "@/components/ui/data-table"
import type { DataTableColumnDef } from "@/components/ui/data-table"
import { Progress } from "@/components/ui/progress"
import { Separator } from "@/components/ui/separator"
import { Skeleton } from "@/components/ui/skeleton"
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs"
import {
  averageFileCyclomatic,
  markerTotal,
  rankedFiles,
  rankedFindings,
  sourceLanguageRows,
} from "@/lib/dashboard-data"
import { parseDashboardTab, type DashboardTab } from "@/lib/dashboard-routes"
import {
  formatCompact,
  formatNumber,
  formatPercent,
  formatRatio,
  formatScore,
} from "@/lib/format"
import type {
  DuplicateBlock,
  FileReport,
  FindingRecord,
  FunctionHotspot,
  LanguageStat,
  RiskEntry,
  ScanReport,
} from "@/lib/types"

const RepositoryGraph = lazy(() =>
  import("@/components/repository-graph").then((module) => ({
    default: module.RepositoryGraph,
  }))
)

export function ReportDashboard({
  report,
  revision,
  activeTab,
  onActiveTabChange,
}: {
  report: ScanReport
  revision: number
  activeTab: DashboardTab
  onActiveTabChange: (tab: DashboardTab) => void
}) {
  const { summary } = report
  const duplicationEnabled =
    report.analysis_profile?.analyzers.duplication ?? true
  const churnEnabled = report.analysis_profile?.analyzers.churn ?? true
  const source = summary.source ?? {
    files: summary.files,
    bytes: summary.bytes,
    tokens: summary.tokens,
    loc: summary.loc,
    sloc: summary.sloc,
    comment_lines: summary.comment_lines,
  }
  const sourceCommentRatio =
    source.loc > 0 ? source.comment_lines / source.loc : 0
  const files = useMemo(() => rankedFiles(report), [report])
  const findings = useMemo(() => rankedFindings(report), [report])
  const tokenBudgetDetail = summary.assessment.token_budget
    ? `${formatPercent((summary.tokens / summary.assessment.token_budget) * 100)} of context budget`
    : "Context budget unavailable"

  const metrics = [
    {
      label: "Files",
      value: formatNumber(summary.files),
      detail: `${formatNumber(source.files)} source · ${formatCompact(summary.bytes)} bytes`,
    },
    {
      label: "Tokens",
      value: formatCompact(summary.tokens),
      detail: tokenBudgetDetail,
    },
    {
      label: "Source lines",
      value: formatCompact(source.sloc),
      detail: formatRatio(sourceCommentRatio) + " comments",
    },
    {
      label: "Duplication",
      value: duplicationEnabled
        ? formatPercent(summary.duplication.duplicated_pct)
        : "Not run",
      detail: duplicationEnabled
        ? `${formatNumber(summary.duplication.duplicated_lines)} / ${formatNumber(
            summary.duplication.analyzed_lines ?? summary.loc
          )} analyzed lines`
        : "Available in full profile",
    },
    {
      label: "Max complexity",
      value: formatNumber(summary.complexity.cyclomatic_max),
      detail:
        formatNumber(summary.complexity.functions_over_threshold) +
        " over limit",
    },
    {
      label: "Maintainability",
      value: summary.complexity.mi_avg.toFixed(1),
      detail: "minimum " + summary.complexity.mi_min.toFixed(1),
    },
  ]

  return (
    <div className="space-y-4">
      <section className="grid grid-cols-2 gap-3 lg:grid-cols-3 xl:grid-cols-6">
        {metrics.map((metric) => (
          <Card key={metric.label}>
            <CardHeader className="gap-1 py-4">
              <CardDescription>{metric.label}</CardDescription>
              <CardTitle className="text-2xl tabular-nums">
                {metric.value}
              </CardTitle>
              <p className="text-xs text-muted-foreground">{metric.detail}</p>
            </CardHeader>
          </Card>
        ))}
      </section>

      <Tabs
        value={activeTab}
        onValueChange={(value) => {
          const tab = parseDashboardTab(value)
          if (tab) onActiveTabChange(tab)
        }}
      >
        <TabsList className="grid h-auto w-full grid-cols-2 sm:grid-cols-4 lg:inline-flex lg:h-9 lg:w-fit">
          <TabsTrigger value="overview">Overview</TabsTrigger>
          <TabsTrigger value="risk">Risk</TabsTrigger>
          <TabsTrigger value="complexity">Complexity</TabsTrigger>
          <TabsTrigger value="duplication">Duplication</TabsTrigger>
          <TabsTrigger value="files">Files</TabsTrigger>
          <TabsTrigger value="findings">Findings</TabsTrigger>
          <TabsTrigger value="graph">
            <Network /> Graph
          </TabsTrigger>
        </TabsList>

        <TabsContent value="overview">
          <Overview report={report} />
        </TabsContent>
        <TabsContent value="risk">
          <RiskTable report={report} churnEnabled={churnEnabled} />
        </TabsContent>
        <TabsContent value="complexity">
          <ComplexityTable report={report} />
        </TabsContent>
        <TabsContent value="duplication">
          <DuplicationTable report={report} enabled={duplicationEnabled} />
        </TabsContent>
        <TabsContent value="files">
          <FilesTable files={files} total={report.files.length} />
        </TabsContent>
        <TabsContent value="findings">
          <FindingsTable
            findings={findings}
            total={report.finding_catalog.findings.length}
          />
        </TabsContent>
        <TabsContent value="graph">
          {activeTab === "graph" ? (
            <Suspense fallback={<Skeleton className="h-[46rem] w-full" />}>
              <RepositoryGraph revision={revision} report={report} />
            </Suspense>
          ) : null}
        </TabsContent>
      </Tabs>
    </div>
  )
}

function languageColumns(
  largestTokens: number
): DataTableColumnDef<LanguageStat>[] {
  return [
    {
      accessorKey: "name",
      header: ({ column }) => (
        <DataTableColumnHeader column={column} title="Language" />
      ),
      cell: ({ row }) => (
        <span className="font-medium">{row.original.name}</span>
      ),
      meta: { label: "Language" },
    },
    {
      accessorKey: "files",
      header: ({ column }) => (
        <DataTableColumnHeader column={column} title="Files" align="right" />
      ),
      cell: ({ row }) => formatNumber(row.original.files),
      meta: { label: "Files", cellClassName: "text-right tabular-nums" },
    },
    {
      accessorKey: "sloc",
      header: ({ column }) => (
        <DataTableColumnHeader column={column} title="SLOC" align="right" />
      ),
      cell: ({ row }) => formatNumber(row.original.sloc),
      meta: { label: "SLOC", cellClassName: "text-right tabular-nums" },
    },
    {
      accessorKey: "tokens",
      header: ({ column }) => (
        <DataTableColumnHeader column={column} title="Tokens" align="right" />
      ),
      cell: ({ row }) => formatNumber(row.original.tokens),
      meta: { label: "Tokens", cellClassName: "text-right tabular-nums" },
    },
    {
      id: "share",
      accessorFn: (language) => language.tokens / largestTokens,
      header: ({ column }) => (
        <DataTableColumnHeader column={column} title="Share" />
      ),
      cell: ({ row }) => (
        <Progress value={(row.original.tokens / largestTokens) * 100} />
      ),
      meta: {
        label: "Share",
        headerClassName: "w-[28%]",
        cellClassName: "min-w-40",
      },
    },
  ]
}

const riskColumns: DataTableColumnDef<RiskEntry>[] = [
  {
    accessorKey: "path",
    header: ({ column }) => (
      <DataTableColumnHeader column={column} title="Path" />
    ),
    cell: ({ row }) => (
      <span
        className="block max-w-[34rem] truncate font-mono text-xs"
        title={row.original.path}
      >
        {row.original.path}
      </span>
    ),
    meta: { label: "Path" },
  },
  {
    accessorKey: "score",
    header: ({ column }) => (
      <DataTableColumnHeader column={column} title="Risk" align="right" />
    ),
    cell: ({ row }) => formatScore(row.original.score),
    meta: {
      label: "Risk",
      cellClassName: "text-right font-medium tabular-nums",
    },
  },
  {
    accessorKey: "sloc",
    header: ({ column }) => (
      <DataTableColumnHeader column={column} title="SLOC" align="right" />
    ),
    cell: ({ row }) => formatNumber(row.original.sloc),
    meta: { label: "SLOC", cellClassName: "text-right tabular-nums" },
  },
  {
    accessorKey: "cyclomatic",
    header: ({ column }) => (
      <DataTableColumnHeader column={column} title="Complexity" align="right" />
    ),
    cell: ({ row }) => formatNumber(row.original.cyclomatic),
    meta: { label: "Complexity", cellClassName: "text-right tabular-nums" },
  },
  {
    accessorKey: "churn_commits",
    header: ({ column }) => (
      <DataTableColumnHeader column={column} title="Commits" align="right" />
    ),
    cell: ({ row }) => formatNumber(row.original.churn_commits),
    meta: { label: "Commits", cellClassName: "text-right tabular-nums" },
  },
  {
    id: "signals",
    accessorFn: (risk) => risk.reasons.join(", "),
    header: ({ column }) => (
      <DataTableColumnHeader column={column} title="Signals" />
    ),
    cell: ({ row }) => (
      <span
        className="block max-w-[28rem] truncate text-muted-foreground"
        title={row.original.reasons.join(", ")}
      >
        {row.original.reasons.join(", ")}
      </span>
    ),
    meta: { label: "Signals" },
  },
]

const complexityColumns: DataTableColumnDef<FunctionHotspot>[] = [
  {
    accessorKey: "name",
    header: ({ column }) => (
      <DataTableColumnHeader column={column} title="Function" />
    ),
    cell: ({ row }) => <span className="font-medium">{row.original.name}</span>,
    meta: { label: "Function" },
  },
  {
    id: "location",
    accessorFn: (fn) => `${fn.path}:${fn.line}`,
    header: ({ column }) => (
      <DataTableColumnHeader column={column} title="Location" />
    ),
    cell: ({ row }) => {
      const location = `${row.original.path}:${row.original.line}`
      return (
        <span
          className="block max-w-[40rem] truncate font-mono text-xs"
          title={location}
        >
          {location}
        </span>
      )
    },
    meta: { label: "Location" },
  },
  {
    accessorKey: "cyclomatic",
    header: ({ column }) => (
      <DataTableColumnHeader column={column} title="Cyclomatic" align="right" />
    ),
    meta: { label: "Cyclomatic", cellClassName: "text-right tabular-nums" },
  },
  {
    accessorKey: "cognitive",
    header: ({ column }) => (
      <DataTableColumnHeader column={column} title="Cognitive" align="right" />
    ),
    meta: { label: "Cognitive", cellClassName: "text-right tabular-nums" },
  },
  {
    accessorKey: "max_nesting",
    header: ({ column }) => (
      <DataTableColumnHeader column={column} title="Nesting" align="right" />
    ),
    meta: { label: "Nesting", cellClassName: "text-right tabular-nums" },
  },
]

const duplicateColumns: DataTableColumnDef<DuplicateBlock>[] = [
  {
    id: "kind",
    accessorFn: (duplicate) =>
      duplicate.similarity === 1 ? "Exact" : "Type-2",
    header: ({ column }) => (
      <DataTableColumnHeader column={column} title="Kind" />
    ),
    cell: ({ row }) => {
      const exact = row.original.similarity === 1
      return (
        <Badge variant={exact ? "secondary" : "outline"}>
          {exact ? "Exact" : "Type-2"}
        </Badge>
      )
    },
    meta: { label: "Kind" },
  },
  {
    accessorKey: "similarity",
    header: ({ column }) => (
      <DataTableColumnHeader column={column} title="Similarity" align="right" />
    ),
    cell: ({ row }) => formatRatio(row.original.similarity),
    meta: { label: "Similarity", cellClassName: "text-right tabular-nums" },
  },
  {
    accessorKey: "lines",
    header: ({ column }) => (
      <DataTableColumnHeader column={column} title="Lines" align="right" />
    ),
    cell: ({ row }) => formatNumber(row.original.lines),
    meta: { label: "Lines", cellClassName: "text-right tabular-nums" },
  },
  {
    accessorKey: "tokens",
    header: ({ column }) => (
      <DataTableColumnHeader column={column} title="Tokens" align="right" />
    ),
    cell: ({ row }) => formatNumber(row.original.tokens),
    meta: { label: "Tokens", cellClassName: "text-right tabular-nums" },
  },
  {
    accessorKey: "copies",
    header: ({ column }) => (
      <DataTableColumnHeader column={column} title="Copies" align="right" />
    ),
    cell: ({ row }) => formatNumber(row.original.copies),
    meta: { label: "Copies", cellClassName: "text-right tabular-nums" },
  },
  {
    accessorKey: "duplicated_lines",
    header: ({ column }) => (
      <DataTableColumnHeader
        column={column}
        title="Removable lines"
        align="right"
      />
    ),
    cell: ({ row }) => formatNumber(row.original.duplicated_lines),
    meta: {
      label: "Removable lines",
      cellClassName: "text-right font-medium tabular-nums",
    },
  },
  {
    id: "locations",
    accessorFn: (duplicate) => duplicate.locations.join(" "),
    header: ({ column }) => (
      <DataTableColumnHeader column={column} title="Locations" />
    ),
    cell: ({ row }) => (
      <div className="space-y-1">
        {row.original.locations.map((location) => (
          <div key={location}>{location}</div>
        ))}
      </div>
    ),
    meta: {
      label: "Locations",
      cellClassName: "min-w-80 whitespace-normal font-mono text-xs",
    },
  },
]

const fileColumns: DataTableColumnDef<FileReport>[] = [
  {
    accessorKey: "path",
    header: ({ column }) => (
      <DataTableColumnHeader column={column} title="Path" />
    ),
    cell: ({ row }) => (
      <span
        className="block max-w-[42rem] truncate font-mono text-xs"
        title={row.original.path}
      >
        {row.original.path}
      </span>
    ),
    meta: { label: "Path" },
  },
  {
    accessorKey: "language",
    header: ({ column }) => (
      <DataTableColumnHeader column={column} title="Language" />
    ),
    meta: { label: "Language" },
  },
  {
    accessorKey: "tokens",
    header: ({ column }) => (
      <DataTableColumnHeader column={column} title="Tokens" align="right" />
    ),
    cell: ({ row }) => formatNumber(row.original.tokens),
    meta: { label: "Tokens", cellClassName: "text-right tabular-nums" },
  },
  {
    accessorKey: "sloc",
    header: ({ column }) => (
      <DataTableColumnHeader column={column} title="SLOC" align="right" />
    ),
    cell: ({ row }) => formatNumber(row.original.sloc),
    meta: { label: "SLOC", cellClassName: "text-right tabular-nums" },
  },
  {
    id: "complexity",
    accessorFn: (file) => file.complexity?.cyclomatic,
    header: ({ column }) => (
      <DataTableColumnHeader
        column={column}
        title="Cyclomatic total"
        align="right"
      />
    ),
    cell: ({ row }) => row.original.complexity?.cyclomatic ?? "-",
    sortUndefined: "last",
    meta: {
      label: "Cyclomatic total",
      cellClassName: "text-right tabular-nums",
    },
  },
  {
    id: "complexity_avg",
    accessorFn: averageFileCyclomatic,
    header: ({ column }) => (
      <DataTableColumnHeader
        column={column}
        title="Cyclomatic avg"
        align="right"
      />
    ),
    cell: ({ row }) => averageFileCyclomatic(row.original)?.toFixed(1) ?? "-",
    sortUndefined: "last",
    meta: { label: "Cyclomatic avg", cellClassName: "text-right tabular-nums" },
  },
  {
    id: "commits",
    accessorFn: (file) => file.churn?.commits,
    header: ({ column }) => (
      <DataTableColumnHeader column={column} title="Commits" align="right" />
    ),
    cell: ({ row }) => row.original.churn?.commits ?? "-",
    sortUndefined: "last",
    meta: { label: "Commits", cellClassName: "text-right tabular-nums" },
  },
]

const severityRank: Record<string, number> = { error: 3, warning: 2, note: 1 }

const findingColumns: DataTableColumnDef<FindingRecord>[] = [
  {
    accessorKey: "severity",
    header: ({ column }) => (
      <DataTableColumnHeader column={column} title="Severity" />
    ),
    cell: ({ row }) => (
      <Badge
        variant={row.original.severity === "error" ? "destructive" : "outline"}
      >
        {row.original.severity}
      </Badge>
    ),
    sortFn: (left, right) =>
      (severityRank[left.original.severity] ?? 0) -
      (severityRank[right.original.severity] ?? 0),
    meta: { label: "Severity" },
  },
  {
    accessorKey: "kind",
    header: ({ column }) => (
      <DataTableColumnHeader column={column} title="Kind" />
    ),
    cell: ({ row }) => <span className="capitalize">{row.original.kind}</span>,
    meta: { label: "Kind" },
  },
  {
    accessorKey: "message",
    header: ({ column }) => (
      <DataTableColumnHeader column={column} title="Message" />
    ),
    cell: ({ row }) => (
      <span
        className="block max-w-[40rem] truncate"
        title={row.original.message}
      >
        {row.original.message}
      </span>
    ),
    meta: { label: "Message" },
  },
  {
    id: "location",
    accessorFn: (finding) =>
      `${finding.primary_location.path}:${finding.primary_location.start_line}`,
    header: ({ column }) => (
      <DataTableColumnHeader column={column} title="Location" />
    ),
    cell: ({ row }) => {
      const location = `${row.original.primary_location.path}:${row.original.primary_location.start_line}`
      return (
        <span
          className="block max-w-[28rem] truncate font-mono text-xs"
          title={location}
        >
          {location}
        </span>
      )
    },
    meta: { label: "Location" },
  },
]

function Overview({ report }: { report: ScanReport }) {
  const { summary, diagnostics } = report
  const languages = useMemo(
    () => sourceLanguageRows(summary.languages),
    [summary.languages]
  )
  const largestLanguageTokens = Math.max(
    1,
    ...languages.map((language) => language.tokens)
  )
  const columns = useMemo(
    () => languageColumns(largestLanguageTokens),
    [largestLanguageTokens]
  )
  const markers = markerTotal(summary.markers)

  return (
    <div className="grid grid-cols-[minmax(0,1fr)] gap-4 lg:grid-cols-[minmax(0,2fr)_minmax(280px,1fr)]">
      <Card>
        <CardHeader>
          <CardTitle>Languages</CardTitle>
          <CardDescription>
            Source composition with non-source inventory collapsed.
          </CardDescription>
        </CardHeader>
        <CardContent>
          <DataTable
            columns={columns}
            data={languages}
            label="Repository languages"
            searchPlaceholder="Search languages..."
            searchText={(language) => language.name}
            emptyMessage="No language data is available."
            initialSorting={[{ id: "tokens", desc: true }]}
            defaultPageSize={10}
            getRowId={(language) => language.name}
          />
        </CardContent>
      </Card>

      <div className="space-y-4">
        <Card>
          <CardHeader>
            <CardTitle>Coverage signals</CardTitle>
          </CardHeader>
          <CardContent className="space-y-4 text-sm">
            <StatLine
              icon={FileCode2}
              label="Analyzed files"
              value={formatNumber(diagnostics.analyzed_files)}
            />
            <StatLine
              icon={TestTube2}
              label="Test files"
              value={formatNumber(summary.test_presence.test_files)}
            />
            <StatLine
              icon={AlertTriangle}
              label="No matching test"
              value={formatNumber(summary.test_presence.untested_source_files)}
            />
            <StatLine
              icon={Activity}
              label="Markers"
              value={formatNumber(markers)}
            />
            <Separator />
            <StatLine
              icon={Gauge}
              label="Cleanup priority"
              value={summary.assessment.cleanup_worth}
            />
            <StatLine
              icon={CheckCircle2}
              label="Fits context budget"
              value={summary.assessment.fits_context ? "Yes" : "No"}
            />
          </CardContent>
        </Card>

        <Card>
          <CardHeader>
            <CardTitle>Diagnostics</CardTitle>
          </CardHeader>
          <CardContent className="space-y-3 text-sm">
            <StatLine
              icon={CircleDot}
              label="Discovered"
              value={formatNumber(diagnostics.discovered_files)}
            />
            <StatLine
              icon={AlertTriangle}
              label="Unsupported"
              value={formatNumber(diagnostics.unsupported_files)}
            />
            <StatLine
              icon={AlertTriangle}
              label="Unreadable"
              value={formatNumber(diagnostics.unreadable_files)}
            />
            <StatLine
              icon={AlertTriangle}
              label="Walker errors"
              value={formatNumber(diagnostics.walker_errors)}
            />
          </CardContent>
        </Card>
      </div>
    </div>
  )
}

function StatLine({
  icon: Icon,
  label,
  value,
}: {
  icon: typeof Activity
  label: string
  value: string
}) {
  return (
    <div className="flex items-center justify-between gap-4">
      <span className="inline-flex items-center gap-2 text-muted-foreground">
        <Icon className="size-4" /> {label}
      </span>
      <span className="font-medium capitalize tabular-nums">{value}</span>
    </div>
  )
}

function RiskTable({
  report,
  churnEnabled,
}: {
  report: ScanReport
  churnEnabled: boolean
}) {
  const algorithmVersion = report.summary.top_risks[0]?.algorithm_version
  const algorithm = algorithmVersion
    ? ` Risk algorithm ${algorithmVersion}.`
    : ""
  return (
    <Card>
      <CardHeader>
        <CardTitle>Highest-risk source files</CardTitle>
        <CardDescription>
          {churnEnabled
            ? `Composite size, complexity, and churn signals.${algorithm}`
            : `Composite size and complexity signals; churn was not run.${algorithm}`}
        </CardDescription>
      </CardHeader>
      <CardContent>
        <DataTable
          columns={riskColumns}
          data={report.summary.top_risks}
          label="Highest-risk source files"
          searchPlaceholder="Search paths or risk signals..."
          searchText={(risk) => `${risk.path} ${risk.reasons.join(" ")}`}
          emptyMessage="No risk entries are available."
          initialSorting={[{ id: "score", desc: true }]}
          getRowId={(risk) => risk.path}
        />
      </CardContent>
    </Card>
  )
}

function ComplexityTable({ report }: { report: ScanReport }) {
  return (
    <Card>
      <CardHeader>
        <CardTitle>Most complex functions</CardTitle>
        <CardDescription>
          {formatNumber(report.summary.complexity.functions)} callable scopes
          analyzed.
        </CardDescription>
      </CardHeader>
      <CardContent>
        <DataTable
          columns={complexityColumns}
          data={report.summary.top_functions}
          label="Most complex functions"
          searchPlaceholder="Search functions or locations..."
          searchText={(fn) => `${fn.name} ${fn.path}:${fn.line}`}
          emptyMessage="No function-level complexity data is available."
          initialSorting={[{ id: "cyclomatic", desc: true }]}
          getRowId={(fn) => `${fn.path}:${fn.line}:${fn.name}`}
        />
      </CardContent>
    </Card>
  )
}

function DuplicationTable({
  report,
  enabled,
}: {
  report: ScanReport
  enabled: boolean
}) {
  const { duplication, top_duplicates, top_production_duplicates } =
    report.summary
  const usesProductionProjection =
    report.summary.assessment.production_duplication !== undefined
  const production = report.summary.assessment.production_duplication
  const duplicates = usesProductionProjection
    ? (top_production_duplicates ?? [])
    : top_duplicates
  const title = usesProductionProjection
    ? "Largest production duplicate blocks"
    : "Largest duplicate blocks"
  const description = !enabled
    ? "Duplication analysis was not run for this report."
    : production
      ? `${production.complete ? "" : "Partial evidence: "}${formatPercent(production.duplicated_pct)} production-source duplication (${formatNumber(production.duplicated_lines)} / ${formatNumber(production.analyzed_lines)} lines); raw detection retained ${formatNumber(duplication.exact_groups)} exact and ${formatNumber(duplication.near_groups)} Type-2 groups.`
      : `${formatNumber(duplication.exact_groups)} exact and ${formatNumber(duplication.near_groups)} Type-2 groups; ${formatNumber(duplication.duplicated_lines)} duplicated lines across the repository.`

  return (
    <Card>
      <CardHeader>
        <CardTitle>{title}</CardTitle>
        <CardDescription>{description}</CardDescription>
      </CardHeader>
      <CardContent>
        {!enabled ? (
          <p className="text-sm text-muted-foreground">
            No duplication data is available.
          </p>
        ) : duplicates.length === 0 ? (
          <p className="text-sm text-muted-foreground">
            No duplicate blocks met the configured thresholds.
          </p>
        ) : (
          <DataTable
            columns={duplicateColumns}
            data={duplicates}
            label={title}
            searchPlaceholder="Search duplicate locations or kind..."
            searchText={(duplicate) =>
              `${duplicate.similarity === 1 ? "exact" : "type-2"} ${duplicate.locations.join(" ")}`
            }
            emptyMessage="No duplicate blocks match this search."
            initialSorting={[{ id: "duplicated_lines", desc: true }]}
            getRowId={(duplicate, index) =>
              `${duplicate.locations[0] ?? "duplicate"}:${index}`
            }
          />
        )}
      </CardContent>
    </Card>
  )
}

function FilesTable({
  files,
  total,
}: {
  files: ScanReport["files"]
  total: number
}) {
  return (
    <Card>
      <CardHeader>
        <CardTitle>Repository files</CardTitle>
        <CardDescription>
          {formatNumber(total)} analyzed files, initially ranked by tokens.
        </CardDescription>
      </CardHeader>
      <CardContent>
        <DataTable
          columns={fileColumns}
          data={files}
          label="Repository files"
          searchPlaceholder="Search paths or languages..."
          searchText={(file) => `${file.path} ${file.language}`}
          emptyMessage="No files match this search."
          initialSorting={[{ id: "tokens", desc: true }]}
          getRowId={(file) => file.path}
        />
      </CardContent>
    </Card>
  )
}

function FindingsTable({
  findings,
  total,
}: {
  findings: ScanReport["finding_catalog"]["findings"]
  total: number
}) {
  return (
    <Card>
      <CardHeader>
        <CardTitle>Canonical findings</CardTitle>
        <CardDescription>
          {formatNumber(total)} ranked findings.
        </CardDescription>
      </CardHeader>
      <CardContent>
        <DataTable
          columns={findingColumns}
          data={findings}
          label="Canonical findings"
          searchPlaceholder="Search findings, messages, or paths..."
          searchText={(finding) =>
            `${finding.severity} ${finding.kind} ${finding.message} ${finding.primary_location.path}:${finding.primary_location.start_line}`
          }
          emptyMessage="No findings match this search."
          getRowId={(finding, index) =>
            `${finding.fingerprint}:${finding.primary_location.path}:${finding.primary_location.start_line}:${index}`
          }
        />
      </CardContent>
    </Card>
  )
}
