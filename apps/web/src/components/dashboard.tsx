import { useEffect, useState } from "react"
import {
  AlertTriangle,
  CheckCircle2,
  CircleDot,
  Clock3,
  LoaderCircle,
  Radio,
  RefreshCw,
} from "lucide-react"

import reposcoutLogo from "@/assets/reposcout.png"
import { ReportDashboard } from "@/components/dashboard-report"
import { ModeToggle } from "@/components/mode-toggle"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import {
  Card,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card"
import { Skeleton } from "@/components/ui/skeleton"
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip"
import type { ConnectionState } from "@/hooks/use-daemon"
import type { DashboardTab } from "@/lib/dashboard-routes"
import { formatDateTime, formatElapsed } from "@/lib/format"
import type { DaemonSnapshot, ScanReport } from "@/lib/types"

interface DashboardProps {
  snapshot: DaemonSnapshot | null
  connection: ConnectionState
  loading: boolean
  error: string | null
  onRescan: () => Promise<void>
  activeTab: DashboardTab
  onActiveTabChange: (tab: DashboardTab) => void
}

export function Dashboard(props: DashboardProps) {
  const scanning = isScanning(props.snapshot)
  const now = useScanClock(scanning)
  const elapsed = scanning
    ? formatElapsed(props.snapshot?.scan_started_at ?? null, now)
    : null

  return (
    <div className="min-h-screen bg-background">
      <DashboardHeader
        snapshot={props.snapshot}
        connection={props.connection}
        scanning={scanning}
        onRescan={props.onRescan}
      />
      <DashboardContent {...props} elapsed={elapsed} />
    </div>
  )
}

function isScanning(snapshot: DaemonSnapshot | null): boolean {
  return snapshot?.status === "scanning" || snapshot?.status === "starting"
}

function useScanClock(active: boolean): number {
  const [now, setNow] = useState(() => Date.now())
  useEffect(() => {
    if (!active) return undefined
    const timer = window.setInterval(() => setNow(Date.now()), 1000)
    return () => window.clearInterval(timer)
  }, [active])
  return now
}

function DashboardHeader({
  snapshot,
  connection,
  scanning,
  onRescan,
}: {
  snapshot: DaemonSnapshot | null
  connection: ConnectionState
  scanning: boolean
  onRescan: () => Promise<void>
}) {
  return (
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
  )
}

function DashboardContent({
  snapshot,
  loading,
  error,
  activeTab,
  onActiveTabChange,
  elapsed,
}: DashboardProps & { elapsed: string | null }) {
  const report = snapshot?.report ?? null
  return (
    <main className="mx-auto max-w-[1600px] space-y-4 px-4 py-4 sm:px-6 sm:py-6">
      <ScanLineStatus
        snapshot={snapshot}
        report={report}
        elapsed={elapsed}
        error={error}
      />
      {loading && !report ? <DashboardSkeleton /> : null}
      {!loading && !report ? (
        <EmptyState error={error ?? snapshot?.error ?? null} />
      ) : null}
      {report ? (
        <ReportDashboard
          report={report}
          revision={snapshot?.revision ?? 0}
          activeTab={activeTab}
          onActiveTabChange={onActiveTabChange}
        />
      ) : null}
    </main>
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
  if (isScanning(snapshot)) {
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
          <CheckCircle2 className="size-3.5" /> Last report{" "}
          {formatDateTime(report.generated_at)}
        </span>
      ) : null}
      {snapshot ? (
        <Badge variant="secondary">{snapshot.profile} profile</Badge>
      ) : null}
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
        <CardDescription>
          {error ?? "The first repository scan is still pending."}
        </CardDescription>
      </CardHeader>
    </Card>
  )
}
