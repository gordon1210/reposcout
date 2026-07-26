import { useState } from "react"
import { render, screen, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { describe, expect, it, vi } from "vitest"

import { Dashboard } from "@/components/dashboard"
import { ThemeProvider } from "@/components/theme-provider"
import { TooltipProvider } from "@/components/ui/tooltip"
import type { DashboardTab } from "@/lib/dashboard-routes"
import type { DaemonSnapshot } from "@/lib/types"
import { makeReport, makeSnapshot } from "@/test/fixtures"

vi.mock("@/components/repository-graph", () => ({
  RepositoryGraph: ({ revision, report }: { revision: number; report: { root: string } }) => (
    <div data-report-root={report.root}>Graph for revision {revision}</div>
  ),
}))

function DashboardHarness({
  snapshot,
  loading,
  onRescan,
  initialTab,
}: {
  snapshot: DaemonSnapshot
  loading: boolean
  onRescan: () => Promise<void>
  initialTab: DashboardTab
}) {
  const [activeTab, setActiveTab] = useState<DashboardTab>(initialTab)

  return (
    <Dashboard
      snapshot={snapshot}
      connection="live"
      loading={loading}
      error={null}
      onRescan={onRescan}
      activeTab={activeTab}
      onActiveTabChange={setActiveTab}
    />
  )
}

function renderDashboard(
  snapshot = makeSnapshot(),
  loading = false,
  initialTab: DashboardTab = "overview",
) {
  const onRescan = vi.fn().mockResolvedValue(undefined)
  render(
    <ThemeProvider>
      <TooltipProvider>
        <DashboardHarness
          snapshot={snapshot}
          loading={loading}
          onRescan={onRescan}
          initialTab={initialTab}
        />
      </TooltipProvider>
    </ThemeProvider>,
  )
  return { onRescan }
}

describe("Dashboard", () => {
  it("shows a bounded loading state before the first report", () => {
    renderDashboard(
      makeSnapshot({ status: "starting", report: null, scan_started_at: null }),
      true,
    )

    expect(screen.getByRole("heading", { name: "RepoScout" })).toBeInTheDocument()
    expect(screen.getByRole("img", { name: "RepoScout" })).toBeInTheDocument()
    expect(screen.getByLabelText("Loading repository metrics")).toBeInTheDocument()
    expect(screen.getByRole("button", { name: "Run repository scan" })).toBeDisabled()
  })

  it("keeps the previous report visible during a long scan", () => {
    renderDashboard(
      makeSnapshot({
        status: "scanning",
        scan_started_at: new Date(Date.now() - 65_000).toISOString(),
      }),
    )

    expect(screen.getByText("Scanning")).toBeInTheDocument()
    expect(screen.getByText(/Current scan 1m/)).toBeInTheDocument()
    expect(screen.getByText("1.2K")).toBeInTheDocument()
    expect(screen.getByText("0.6% of context budget")).toBeInTheDocument()
    expect(screen.queryByText("1,234 total")).not.toBeInTheDocument()
    expect(screen.getByText("Not run")).toBeInTheDocument()
    expect(screen.getByText("lite profile")).toBeInTheDocument()
    expect(screen.getByRole("button", { name: "Run repository scan" })).toBeDisabled()
  })

  it("navigates to risk details without replacing the report", async () => {
    const user = userEvent.setup()
    renderDashboard()

    await user.click(screen.getByRole("tab", { name: "Risk" }))

    expect(screen.getByText("Highest-risk source files")).toBeInTheDocument()
    expect(screen.getByText("src/lib.rs")).toBeInTheDocument()
    expect(screen.getByText("high complexity")).toBeInTheDocument()
  })

  it("shows total and average cyclomatic complexity for each file", () => {
    const report = makeReport()
    report.files[0].complexity = {
      ...report.files[0].complexity!,
      cyclomatic: 9,
      functions: [
        { name: "first", line: 1, cyclomatic: 3, cognitive: 2, max_nesting: 1 },
        { name: "second", line: 10, cyclomatic: 5, cognitive: 4, max_nesting: 2 },
      ],
    }

    renderDashboard(makeSnapshot({ report }), false, "files")

    const table = screen.getByRole("table", { name: "Repository files" })
    expect(within(table).getByRole("button", { name: "Sort by Cyclomatic total" })).toBeInTheDocument()
    expect(within(table).getByRole("button", { name: "Sort by Cyclomatic avg" })).toBeInTheDocument()

    const measured = within(table).getByRole("row", { name: /src\/lib\.rs/ })
    expect(within(measured).getByText("9")).toBeInTheDocument()
    expect(within(measured).getByText("4.0")).toBeInTheDocument()

    const withoutCallables = within(table).getByRole("row", { name: /src\/main\.rs/ })
    expect(within(withoutCallables).getByText("-")).toBeInTheDocument()
  })

  it("collapses non-source language inventory in the overview", () => {
    const report = makeReport()
    report.summary.languages.push({
      name: "JSON",
      source: false,
      files: 3,
      bytes: 12_000,
      loc: 300,
      sloc: 300,
      comment_lines: 0,
      tokens: 4_000,
    })

    renderDashboard(makeSnapshot({ report }))

    expect(screen.getByText("Other content (1 formats)")).toBeInTheDocument()
    expect(screen.queryByText("JSON")).not.toBeInTheDocument()
  })

  it("loads graph analysis only after the Graph tab is opened", async () => {
    const user = userEvent.setup()
    renderDashboard(makeSnapshot({ revision: 12 }))

    expect(screen.getAllByRole("tab").at(-1)).toHaveTextContent("Graph")
    expect(screen.queryByText("Graph for revision 12")).not.toBeInTheDocument()

    await user.click(screen.getByRole("tab", { name: "Graph" }))

    expect(await screen.findByText("Graph for revision 12")).toHaveAttribute("data-report-root", "/workspace/repo")
  })

  it("shows ranked exact and Type-2 duplicate blocks", async () => {
    const user = userEvent.setup()
    const report = makeReport()
    report.analysis_profile!.analyzers.duplication = true
    report.summary.duplication.exact_groups = 1
    report.summary.duplication.near_groups = 1
    report.summary.top_duplicates = [
      {
        lines: 12,
        tokens: 80,
        similarity: 1,
        copies: 3,
        duplicated_lines: 24,
        locations: ["src/a.rs:10-21", "src/b.rs:30-41", "src/c.rs:50-61"],
      },
      {
        lines: 8,
        tokens: 52,
        similarity: 0.92,
        copies: 2,
        duplicated_lines: 8,
        locations: ["src/d.rs:5-12", "src/e.rs:20-27"],
      },
    ]
    renderDashboard(makeSnapshot({ profile: "full", report }))

    await user.click(screen.getByRole("tab", { name: "Duplication" }))

    expect(screen.getByText("Largest duplicate blocks")).toBeInTheDocument()
    expect(screen.getByText("Exact")).toBeInTheDocument()
    expect(screen.getByText("Type-2")).toBeInTheDocument()
    expect(screen.getByText("92.0%")).toBeInTheDocument()
    expect(screen.getByText("src/e.rs:20-27")).toBeInTheDocument()
  })

  it("shows the empty duplication result", async () => {
    const user = userEvent.setup()
    const report = makeReport()
    report.analysis_profile!.analyzers.duplication = true
    renderDashboard(makeSnapshot({ profile: "full", report }))

    await user.click(screen.getByRole("tab", { name: "Duplication" }))

    expect(screen.getByText("No duplicate blocks met the configured thresholds.")).toBeInTheDocument()
  })

  it("shows when duplication was not run", async () => {
    const user = userEvent.setup()
    renderDashboard()

    await user.click(screen.getByRole("tab", { name: "Duplication" }))

    expect(screen.getByText("Duplication analysis was not run for this report.")).toBeInTheDocument()
    expect(screen.getByText("No duplication data is available.")).toBeInTheDocument()
  })

  it("renders findings that share a fingerprint without duplicate React keys", async () => {
    const user = userEvent.setup()
    const report = makeReport()
    const finding = report.finding_catalog.findings[0]
    report.finding_catalog.findings = [
      finding,
      {
        ...finding,
        message: "Same duplicate family at another location",
        primary_location: { path: "src/main.rs", start_line: 24, end_line: 30 },
      },
    ]
    const consoleError = vi.spyOn(console, "error").mockImplementation(() => undefined)

    try {
      renderDashboard(makeSnapshot({ report }))
      await user.click(screen.getByRole("tab", { name: "Findings" }))

      expect(screen.getByText("TODO marker")).toBeInTheDocument()
      expect(screen.getByText("Same duplicate family at another location")).toBeInTheDocument()
      expect(consoleError.mock.calls.flat().join(" ")).not.toContain("same key")
    } finally {
      consoleError.mockRestore()
    }
  })
})
