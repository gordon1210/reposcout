export const DASHBOARD_TABS = [
  "overview",
  "risk",
  "complexity",
  "duplication",
  "files",
  "findings",
  "graph",
] as const

export type DashboardTab = (typeof DASHBOARD_TABS)[number]

const tabLabels: Record<DashboardTab, string> = {
  overview: "Overview",
  risk: "Risk",
  complexity: "Complexity",
  duplication: "Duplication",
  files: "Files",
  findings: "Findings",
  graph: "Graph",
}

export function parseDashboardTab(value: string | undefined): DashboardTab | null {
  return DASHBOARD_TABS.find((tab) => tab === value) ?? null
}

export function dashboardPath(tab: DashboardTab): string {
  return `/${tab}`
}

export function dashboardTitle(tab: DashboardTab): string {
  return `${tabLabels[tab]} · RepoScout`
}
