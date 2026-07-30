import { useEffect } from "react"
import { Navigate, Route, Routes, useNavigate, useParams } from "react-router"

import { Dashboard } from "@/components/dashboard"
import { ThemeProvider } from "@/components/theme-provider"
import { TooltipProvider } from "@/components/ui/tooltip"
import { useDaemon } from "@/hooks/use-daemon"
import {
  dashboardPath,
  dashboardTitle,
  parseDashboardTab,
  type DashboardTab,
} from "@/lib/dashboard-routes"

export function App() {
  const daemon = useDaemon()

  return (
    <ThemeProvider defaultTheme="system">
      <TooltipProvider>
        <Routes>
          <Route
            index
            element={<Navigate to={dashboardPath("overview")} replace />}
          />
          <Route
            path="graph/*"
            element={<RoutedDashboard daemon={daemon} fixedTab="graph" />}
          />
          <Route path=":tab" element={<RoutedDashboard daemon={daemon} />} />
          <Route
            path="*"
            element={<Navigate to={dashboardPath("overview")} replace />}
          />
        </Routes>
      </TooltipProvider>
    </ThemeProvider>
  )
}

function RoutedDashboard({
  daemon,
  fixedTab,
}: {
  daemon: ReturnType<typeof useDaemon>
  fixedTab?: DashboardTab
}) {
  const { tab: routeTab } = useParams()
  const navigate = useNavigate()
  const activeTab = fixedTab ?? parseDashboardTab(routeTab)

  useEffect(() => {
    if (activeTab) document.title = dashboardTitle(activeTab)
  }, [activeTab])

  if (!activeTab) return <Navigate to={dashboardPath("overview")} replace />

  const navigateToTab = (tab: DashboardTab) => navigate(dashboardPath(tab))

  return (
    <Dashboard
      snapshot={daemon.snapshot}
      connection={daemon.connection}
      loading={daemon.loading}
      error={daemon.error}
      onRescan={daemon.rescan}
      activeTab={activeTab}
      onActiveTabChange={navigateToTab}
    />
  )
}
