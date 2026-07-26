import { lazy, Suspense, useEffect, useMemo, useState } from "react"
import type { ColumnDef } from "@tanstack/react-table"
import {
  Activity,
  AlertTriangle,
  CheckCircle2,
  CircleDot,
  Clock3,
  FileCode2,
  Gauge,
  LoaderCircle,
  Network,
  Radio,
  RefreshCw,
  TestTube2,
} from "lucide-react"

import reposcoutLogo from "@/assets/reposcout.png"
import { ModeToggle } from "@/components/mode-toggle"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card"
import { DataTable, DataTableColumnHeader } from "@/components/ui/data-table"
import { Progress } from "@/components/ui/progress"
import { Separator } from "@/components/ui/separator"
import { Skeleton } from "@/components/ui/skeleton"
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs"
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip"
import type { ConnectionState } from "@/hooks/use-daemon"
import {
  averageFileCyclomatic,
  markerTotal,
  rankedFiles,
  rankedFindings,
} from "@/lib/dashboard-data"
import { parseDashboardTab, type DashboardTab } from "@/lib/dashboard-routes"
import {
  formatCompact,
  formatDateTime,
  formatElapsed,
  formatNumber,
  formatPercent,
  formatRatio,
  formatScore,
} from "@/lib/format"
import type {
  DaemonSnapshot,
  DuplicateBlock,
  FileReport,
  FindingRecord,
  FunctionHotspot,
  LanguageStat,
  RiskEntry,
  ScanReport,
} from "@/lib/types"

const RepositoryGraph = lazy(() =>
  import("@/components/repository-graph").then((module) => ({ default: module.RepositoryGraph })),
)

interface DashboardProps {
  snapshot: DaemonSnapshot | null
  connection: ConnectionState
  loading: boolean
  error: string | null
  onRescan: () => Promise<void>
  activeTab: DashboardTab
  onActiveTabChange: (tab: DashboardTab) => void
}

export function Dashboard({
  snapshot,
  connection,
  loading,
  error,
  onRescan,
  activeTab,
  onActiveTabChange,
}: DashboardProps) {
  const report = snapshot?.report ?? null
  const scanning = snapshot?.status === "scanning" || snapshot?.status === "starting"
  const [now, setNow] = useState(Date.now())

  useEffect(() => {
    if (!scanning) return
    setNow(Date.now())
    const timer = window.setInterval(() => setNow(Date.now()), 1000)
    return () => window.clearInterval(timer)
  }, [scanning])

  const elapsed = scanning ? formatElapsed(snapshot?.scan_started_at ?? null, now) : null

  return (
    <div className="min-h-screen bg-background">
      <header className="border-b">
        <div className="mx-auto flex max-w-[1600px] items-center justify-between gap-4 px-4 py-3 sm:px-6">
          <div className="flex min-w-0 items-center gap-3">
            <h1 className="shrink-0">
              <img src={reposcoutLogo} alt="RepoScout" className="h-14 w-auto" />
            </h1>
            <div className="min-w-0">
              <p className="truncate font-mono text-xs text-muted-foreground">
                {snapshot?.target ?? "Waiting for daemon"}
              </p>
            </div>
          </div>
          <div className="flex shrink-0 items-center gap-2">
            <DaemonBadge connection={connection} snapshot={snapshot} />
            <Tooltip>
              <TooltipTrigger asChild>
                <Button
                  variant="outline"
                  size="icon"
                  onClick={() => void onRescan()}
                  disabled={scanning || connection === "offline"}
                  aria-label="Run repository scan"
                >
                  <RefreshCw className={scanning ? "animate-spin" : undefined} />
                </Button>
              </TooltipTrigger>
              <TooltipContent>Run scan</TooltipContent>
            </Tooltip>
            <ModeToggle />
          </div>
        </div>
      </header>

      <main className="mx-auto max-w-[1600px] space-y-4 px-4 py-4 sm:px-6 sm:py-6">
        <ScanLineStatus snapshot={snapshot} report={report} elapsed={elapsed} error={error} />
        {loading && !report ? <DashboardSkeleton /> : null}
        {!loading && !report ? <EmptyState error={error ?? snapshot?.error ?? null} /> : null}
        {report ? (
          <ReportDashboard
            report={report}
            revision={snapshot?.revision ?? 0}
            activeTab={activeTab}
            onActiveTabChange={onActiveTabChange}
          />
        ) : null}
      </main>
    </div>
  )
}

function DaemonBadge({
  connection,
  snapshot,
}: {
  connection: ConnectionState
  snapshot: DaemonSnapshot | null
}) {
  if (connection === "offline") {
    return (
      <Badge variant="destructive">
        <AlertTriangle /> Offline
      </Badge>
    )
  }
  if (snapshot?.status === "error") {
    return (
      <Badge variant="destructive">
        <AlertTriangle /> Scan failed
      </Badge>
    )
  }
  if (snapshot?.status === "scanning" || snapshot?.status === "starting") {
    return (
      <Badge variant="secondary">
        <LoaderCircle className="animate-spin" /> Scanning
      </Badge>
    )
  }
  if (connection === "live") {
    return (
      <Badge variant="outline">
        <Radio /> Live
      </Badge>
    )
  }
  return (
    <Badge variant="secondary">
      <CircleDot /> Connecting
    </Badge>
  )
}

function ScanLineStatus({
  snapshot,
  report,
  elapsed,
  error,
}: {
  snapshot: DaemonSnapshot | null
  report: ScanReport | null
  elapsed: string | null
  error: string | null
}) {
  if (!snapshot && !error) return null
  return (
    <div className="flex flex-wrap items-center gap-x-4 gap-y-1 text-xs text-muted-foreground">
      {report ? (
        <span className="inline-flex items-center gap-1.5">
          <CheckCircle2 className="size-3.5" /> Last report {formatDateTime(report.generated_at)}
        </span>
      ) : null}
      {snapshot ? <Badge variant="secondary">{snapshot.profile} profile</Badge> : null}
      {snapshot?.status === "scanning" ? (
        <span className="inline-flex items-center gap-1.5">
          <Clock3 className="size-3.5" /> Current scan {elapsed ?? "starting"}
        </span>
      ) : null}
      {snapshot?.error || error ? (
        <span className="inline-flex items-center gap-1.5 text-destructive">
          <AlertTriangle className="size-3.5" /> {snapshot?.error ?? error}
        </span>
      ) : null}
    </div>
  )
}

function DashboardSkeleton() {
  return (
    <div className="space-y-4" aria-label="Loading repository metrics">
      <div className="grid gap-3 sm:grid-cols-2 lg:grid-cols-3 xl:grid-cols-6">
        {Array.from({ length: 6 }, (_, index) => (
          <Card key={index}>
            <CardHeader>
              <Skeleton className="h-4 w-20" />
              <Skeleton className="h-7 w-24" />
            </CardHeader>
          </Card>
        ))}
      </div>
      <Skeleton className="h-96 w-full" />
    </div>
  )
}

function EmptyState({ error }: { error: string | null }) {
  return (
    <Card>
      <CardHeader>
        <CardTitle>No report available</CardTitle>
        <CardDescription>{error ?? "The first repository scan is still pending."}</CardDescription>
      </CardHeader>
    </Card>
  )
}

function ReportDashboard({
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
  const duplicationEnabled = report.analysis_profile?.analyzers.duplication ?? true
  const churnEnabled = report.analysis_profile?.analyzers.churn ?? true
  const source = summary.source ?? {
    files: summary.files,
    bytes: summary.bytes,
    tokens: summary.tokens,
    loc: summary.loc,
    sloc: summary.sloc,
    comment_lines: summary.comment_lines,
  }
  const sourceCommentRatio = source.loc > 0 ? source.comment_lines / source.loc : 0
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
    { label: "Tokens", value: formatCompact(summary.tokens), detail: tokenBudgetDetail },
    {
      label: "Source lines",
      value: formatCompact(source.sloc),
      detail: formatRatio(sourceCommentRatio) + " comments",
    },
    {
      label: "Duplication",
      value: duplicationEnabled ? formatPercent(summary.duplication.duplicated_pct) : "Not run",
      detail: duplicationEnabled
        ? `${formatNumber(summary.duplication.duplicated_lines)} / ${formatNumber(
            summary.duplication.analyzed_lines ?? summary.loc,
          )} analyzed lines`
        : "Available in full profile",
    },
    {
      label: "Max complexity",
      value: formatNumber(summary.complexity.cyclomatic_max),
      detail: formatNumber(summary.complexity.functions_over_threshold) + " over limit",
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
              <CardTitle className="text-2xl tabular-nums">{metric.value}</CardTitle>
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
          <TabsTrigger value="graph"><Network /> Graph</TabsTrigger>
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
          <FindingsTable findings={findings} total={report.finding_catalog.findings.length} />
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

function languageColumns(largestTokens: number): ColumnDef<LanguageStat, unknown>[] {
  return [
    {
      accessorKey: "name",
      header: ({ column }) => <DataTableColumnHeader column={column} title="Language" />,
      cell: ({ row }) => <span className="font-medium">{row.original.name}</span>,
      meta: { label: "Language" },
    },
    {
      accessorKey: "files",
      header: ({ column }) => <DataTableColumnHeader column={column} title="Files" align="right" />,
      cell: ({ row }) => formatNumber(row.original.files),
      meta: { label: "Files", cellClassName: "text-right tabular-nums" },
    },
    {
      accessorKey: "sloc",
      header: ({ column }) => <DataTableColumnHeader column={column} title="SLOC" align="right" />,
      cell: ({ row }) => formatNumber(row.original.sloc),
      meta: { label: "SLOC", cellClassName: "text-right tabular-nums" },
    },
    {
      accessorKey: "tokens",
      header: ({ column }) => <DataTableColumnHeader column={column} title="Tokens" align="right" />,
      cell: ({ row }) => formatNumber(row.original.tokens),
      meta: { label: "Tokens", cellClassName: "text-right tabular-nums" },
    },
    {
      id: "share",
      accessorFn: (language) => language.tokens / largestTokens,
      header: ({ column }) => <DataTableColumnHeader column={column} title="Share" />,
      cell: ({ row }) => <Progress value={(row.original.tokens / largestTokens) * 100} />,
      meta: { label: "Share", headerClassName: "w-[28%]", cellClassName: "min-w-40" },
    },
  ]
}

function sourceLanguageRows(languages: LanguageStat[]): LanguageStat[] {
  const source = languages.filter((language) => language.source !== false)
  const content = languages.filter((language) => language.source === false)
  if (content.length === 0) return source

  return [
    ...source,
    content.reduce<LanguageStat>(
      (total, language) => ({
        ...total,
        files: total.files + language.files,
        bytes: total.bytes + language.bytes,
        loc: total.loc + language.loc,
        sloc: total.sloc + language.sloc,
        comment_lines: total.comment_lines + language.comment_lines,
        tokens: total.tokens + language.tokens,
      }),
      {
        name: `Other content (${content.length} formats)`,
        source: false,
        files: 0,
        bytes: 0,
        loc: 0,
        sloc: 0,
        comment_lines: 0,
        tokens: 0,
      },
    ),
  ]
}

const riskColumns: ColumnDef<RiskEntry, unknown>[] = [
  {
    accessorKey: "path",
    header: ({ column }) => <DataTableColumnHeader column={column} title="Path" />,
    cell: ({ row }) => (
      <span className="block max-w-[34rem] truncate font-mono text-xs" title={row.original.path}>
        {row.original.path}
      </span>
    ),
    meta: { label: "Path" },
  },
  {
    accessorKey: "score",
    header: ({ column }) => <DataTableColumnHeader column={column} title="Risk" align="right" />,
    cell: ({ row }) => formatScore(row.original.score),
    meta: { label: "Risk", cellClassName: "text-right font-medium tabular-nums" },
  },
  {
    accessorKey: "sloc",
    header: ({ column }) => <DataTableColumnHeader column={column} title="SLOC" align="right" />,
    cell: ({ row }) => formatNumber(row.original.sloc),
    meta: { label: "SLOC", cellClassName: "text-right tabular-nums" },
  },
  {
    accessorKey: "cyclomatic",
    header: ({ column }) => <DataTableColumnHeader column={column} title="Complexity" align="right" />,
    cell: ({ row }) => formatNumber(row.original.cyclomatic),
    meta: { label: "Complexity", cellClassName: "text-right tabular-nums" },
  },
  {
    accessorKey: "churn_commits",
    header: ({ column }) => <DataTableColumnHeader column={column} title="Commits" align="right" />,
    cell: ({ row }) => formatNumber(row.original.churn_commits),
    meta: { label: "Commits", cellClassName: "text-right tabular-nums" },
  },
  {
    id: "signals",
    accessorFn: (risk) => risk.reasons.join(", "),
    header: ({ column }) => <DataTableColumnHeader column={column} title="Signals" />,
    cell: ({ row }) => (
      <span className="block max-w-[28rem] truncate text-muted-foreground" title={row.original.reasons.join(", ")}>
        {row.original.reasons.join(", ")}
      </span>
    ),
    meta: { label: "Signals" },
  },
]

const complexityColumns: ColumnDef<FunctionHotspot, unknown>[] = [
  {
    accessorKey: "name",
    header: ({ column }) => <DataTableColumnHeader column={column} title="Function" />,
    cell: ({ row }) => <span className="font-medium">{row.original.name}</span>,
    meta: { label: "Function" },
  },
  {
    id: "location",
    accessorFn: (fn) => `${fn.path}:${fn.line}`,
    header: ({ column }) => <DataTableColumnHeader column={column} title="Location" />,
    cell: ({ row }) => {
      const location = `${row.original.path}:${row.original.line}`
      return (
        <span className="block max-w-[40rem] truncate font-mono text-xs" title={location}>
          {location}
        </span>
      )
    },
    meta: { label: "Location" },
  },
  {
    accessorKey: "cyclomatic",
    header: ({ column }) => <DataTableColumnHeader column={column} title="Cyclomatic" align="right" />,
    meta: { label: "Cyclomatic", cellClassName: "text-right tabular-nums" },
  },
  {
    accessorKey: "cognitive",
    header: ({ column }) => <DataTableColumnHeader column={column} title="Cognitive" align="right" />,
    meta: { label: "Cognitive", cellClassName: "text-right tabular-nums" },
  },
  {
    accessorKey: "max_nesting",
    header: ({ column }) => <DataTableColumnHeader column={column} title="Nesting" align="right" />,
    meta: { label: "Nesting", cellClassName: "text-right tabular-nums" },
  },
]

const duplicateColumns: ColumnDef<DuplicateBlock, unknown>[] = [
  {
    id: "kind",
    accessorFn: (duplicate) => (duplicate.similarity === 1 ? "Exact" : "Type-2"),
    header: ({ column }) => <DataTableColumnHeader column={column} title="Kind" />,
    cell: ({ row }) => {
      const exact = row.original.similarity === 1
      return <Badge variant={exact ? "secondary" : "outline"}>{exact ? "Exact" : "Type-2"}</Badge>
    },
    meta: { label: "Kind" },
  },
  {
    accessorKey: "similarity",
    header: ({ column }) => <DataTableColumnHeader column={column} title="Similarity" align="right" />,
    cell: ({ row }) => formatRatio(row.original.similarity),
    meta: { label: "Similarity", cellClassName: "text-right tabular-nums" },
  },
  {
    accessorKey: "lines",
    header: ({ column }) => <DataTableColumnHeader column={column} title="Lines" align="right" />,
    cell: ({ row }) => formatNumber(row.original.lines),
    meta: { label: "Lines", cellClassName: "text-right tabular-nums" },
  },
  {
    accessorKey: "tokens",
    header: ({ column }) => <DataTableColumnHeader column={column} title="Tokens" align="right" />,
    cell: ({ row }) => formatNumber(row.original.tokens),
    meta: { label: "Tokens", cellClassName: "text-right tabular-nums" },
  },
  {
    accessorKey: "copies",
    header: ({ column }) => <DataTableColumnHeader column={column} title="Copies" align="right" />,
    cell: ({ row }) => formatNumber(row.original.copies),
    meta: { label: "Copies", cellClassName: "text-right tabular-nums" },
  },
  {
    accessorKey: "duplicated_lines",
    header: ({ column }) => <DataTableColumnHeader column={column} title="Removable lines" align="right" />,
    cell: ({ row }) => formatNumber(row.original.duplicated_lines),
    meta: { label: "Removable lines", cellClassName: "text-right font-medium tabular-nums" },
  },
  {
    id: "locations",
    accessorFn: (duplicate) => duplicate.locations.join(" "),
    header: ({ column }) => <DataTableColumnHeader column={column} title="Locations" />,
    cell: ({ row }) => (
      <div className="space-y-1">
        {row.original.locations.map((location) => (
          <div key={location}>{location}</div>
        ))}
      </div>
    ),
    meta: { label: "Locations", cellClassName: "min-w-80 whitespace-normal font-mono text-xs" },
  },
]

const fileColumns: ColumnDef<FileReport, unknown>[] = [
  {
    accessorKey: "path",
    header: ({ column }) => <DataTableColumnHeader column={column} title="Path" />,
    cell: ({ row }) => (
      <span className="block max-w-[42rem] truncate font-mono text-xs" title={row.original.path}>
        {row.original.path}
      </span>
    ),
    meta: { label: "Path" },
  },
  {
    accessorKey: "language",
    header: ({ column }) => <DataTableColumnHeader column={column} title="Language" />,
    meta: { label: "Language" },
  },
  {
    accessorKey: "tokens",
    header: ({ column }) => <DataTableColumnHeader column={column} title="Tokens" align="right" />,
    cell: ({ row }) => formatNumber(row.original.tokens),
    meta: { label: "Tokens", cellClassName: "text-right tabular-nums" },
  },
  {
    accessorKey: "sloc",
    header: ({ column }) => <DataTableColumnHeader column={column} title="SLOC" align="right" />,
    cell: ({ row }) => formatNumber(row.original.sloc),
    meta: { label: "SLOC", cellClassName: "text-right tabular-nums" },
  },
  {
    id: "complexity",
    accessorFn: (file) => file.complexity?.cyclomatic,
    header: ({ column }) => <DataTableColumnHeader column={column} title="Cyclomatic total" align="right" />,
    cell: ({ row }) => row.original.complexity?.cyclomatic ?? "-",
    sortUndefined: "last",
    meta: { label: "Cyclomatic total", cellClassName: "text-right tabular-nums" },
  },
  {
    id: "complexity_avg",
    accessorFn: averageFileCyclomatic,
    header: ({ column }) => <DataTableColumnHeader column={column} title="Cyclomatic avg" align="right" />,
    cell: ({ row }) => averageFileCyclomatic(row.original)?.toFixed(1) ?? "-",
    sortUndefined: "last",
    meta: { label: "Cyclomatic avg", cellClassName: "text-right tabular-nums" },
  },
  {
    id: "commits",
    accessorFn: (file) => file.churn?.commits,
    header: ({ column }) => <DataTableColumnHeader column={column} title="Commits" align="right" />,
    cell: ({ row }) => row.original.churn?.commits ?? "-",
    sortUndefined: "last",
    meta: { label: "Commits", cellClassName: "text-right tabular-nums" },
  },
]

const severityRank: Record<string, number> = { error: 3, warning: 2, note: 1 }

const findingColumns: ColumnDef<FindingRecord, unknown>[] = [
  {
    accessorKey: "severity",
    header: ({ column }) => <DataTableColumnHeader column={column} title="Severity" />,
    cell: ({ row }) => (
      <Badge variant={row.original.severity === "error" ? "destructive" : "outline"}>
        {row.original.severity}
      </Badge>
    ),
    sortingFn: (left, right) =>
      (severityRank[left.original.severity] ?? 0) - (severityRank[right.original.severity] ?? 0),
    meta: { label: "Severity" },
  },
  {
    accessorKey: "kind",
    header: ({ column }) => <DataTableColumnHeader column={column} title="Kind" />,
    cell: ({ row }) => <span className="capitalize">{row.original.kind}</span>,
    meta: { label: "Kind" },
  },
  {
    accessorKey: "message",
    header: ({ column }) => <DataTableColumnHeader column={column} title="Message" />,
    cell: ({ row }) => (
      <span className="block max-w-[40rem] truncate" title={row.original.message}>
        {row.original.message}
      </span>
    ),
    meta: { label: "Message" },
  },
  {
    id: "location",
    accessorFn: (finding) => `${finding.primary_location.path}:${finding.primary_location.start_line}`,
    header: ({ column }) => <DataTableColumnHeader column={column} title="Location" />,
    cell: ({ row }) => {
      const location = `${row.original.primary_location.path}:${row.original.primary_location.start_line}`
      return (
        <span className="block max-w-[28rem] truncate font-mono text-xs" title={location}>
          {location}
        </span>
      )
    },
    meta: { label: "Location" },
  },
]

function Overview({ report }: { report: ScanReport }) {
  const { summary, diagnostics } = report
  const languages = useMemo(() => sourceLanguageRows(summary.languages), [summary.languages])
  const largestLanguageTokens = Math.max(1, ...languages.map((language) => language.tokens))
  const columns = useMemo(() => languageColumns(largestLanguageTokens), [largestLanguageTokens])
  const markers = markerTotal(summary.markers)

  return (
    <div className="grid grid-cols-[minmax(0,1fr)] gap-4 lg:grid-cols-[minmax(0,2fr)_minmax(280px,1fr)]">
      <Card>
        <CardHeader>
          <CardTitle>Languages</CardTitle>
          <CardDescription>Source composition with non-source inventory collapsed.</CardDescription>
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
            <StatLine icon={FileCode2} label="Analyzed files" value={formatNumber(diagnostics.analyzed_files)} />
            <StatLine icon={TestTube2} label="Test files" value={formatNumber(summary.test_presence.test_files)} />
            <StatLine
              icon={AlertTriangle}
              label="No matching test"
              value={formatNumber(summary.test_presence.untested_source_files)}
            />
            <StatLine icon={Activity} label="Markers" value={formatNumber(markers)} />
            <Separator />
            <StatLine icon={Gauge} label="Cleanup priority" value={summary.assessment.cleanup_worth} />
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
            <StatLine icon={CircleDot} label="Discovered" value={formatNumber(diagnostics.discovered_files)} />
            <StatLine icon={AlertTriangle} label="Unsupported" value={formatNumber(diagnostics.unsupported_files)} />
            <StatLine icon={AlertTriangle} label="Unreadable" value={formatNumber(diagnostics.unreadable_files)} />
            <StatLine icon={AlertTriangle} label="Walker errors" value={formatNumber(diagnostics.walker_errors)} />
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

function RiskTable({ report, churnEnabled }: { report: ScanReport; churnEnabled: boolean }) {
  return (
    <Card>
      <CardHeader>
        <CardTitle>Highest-risk source files</CardTitle>
        <CardDescription>
          {churnEnabled
            ? "Composite size, complexity, churn, and test-presence signals."
            : "Composite size, complexity, and test-presence signals; churn was not run."}
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
          {formatNumber(report.summary.complexity.functions)} callable scopes analyzed.
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

function DuplicationTable({ report, enabled }: { report: ScanReport; enabled: boolean }) {
  const { duplication, top_duplicates: duplicates } = report.summary

  return (
    <Card>
      <CardHeader>
        <CardTitle>Largest duplicate blocks</CardTitle>
        <CardDescription>
          {enabled
            ? `${formatNumber(duplication.exact_groups)} exact and ${formatNumber(duplication.near_groups)} Type-2 groups; ${formatNumber(duplication.duplicated_lines)} duplicated lines across the repository.`
            : "Duplication analysis was not run for this report."}
        </CardDescription>
      </CardHeader>
      <CardContent>
        {!enabled ? (
          <p className="text-sm text-muted-foreground">No duplication data is available.</p>
        ) : duplicates.length === 0 ? (
          <p className="text-sm text-muted-foreground">
            No duplicate blocks met the configured thresholds.
          </p>
        ) : (
          <DataTable
            columns={duplicateColumns}
            data={duplicates}
            label="Largest duplicate blocks"
            searchPlaceholder="Search duplicate locations or kind..."
            searchText={(duplicate) =>
              `${duplicate.similarity === 1 ? "exact" : "type-2"} ${duplicate.locations.join(" ")}`
            }
            emptyMessage="No duplicate blocks match this search."
            initialSorting={[{ id: "duplicated_lines", desc: true }]}
            getRowId={(duplicate, index) => `${duplicate.locations[0] ?? "duplicate"}:${index}`}
          />
        )}
      </CardContent>
    </Card>
  )
}

function FilesTable({ files, total }: { files: ScanReport["files"]; total: number }) {
  return (
    <Card>
      <CardHeader>
        <CardTitle>Repository files</CardTitle>
        <CardDescription>{formatNumber(total)} analyzed files, initially ranked by tokens.</CardDescription>
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
        <CardDescription>{formatNumber(total)} ranked findings.</CardDescription>
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
