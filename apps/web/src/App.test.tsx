import { render, screen } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { MemoryRouter, useLocation } from "react-router"
import { beforeEach, describe, expect, it, vi } from "vitest"

import { App } from "@/App"
import { makeSnapshot } from "@/test/fixtures"

const { useDaemonMock } = vi.hoisted(() => ({ useDaemonMock: vi.fn() }))

vi.mock("@/hooks/use-daemon", () => ({
  useDaemon: () => useDaemonMock(),
}))

vi.mock("@/components/repository-graph", () => ({
  RepositoryGraph: ({ revision }: { revision: number }) => <div>Graph for revision {revision}</div>,
}))

function LocationProbe() {
  const location = useLocation()
  return <output aria-label="Current route">{location.pathname}{location.search}</output>
}

function renderRoute(path: string) {
  render(
    <MemoryRouter initialEntries={[path]}>
      <App />
      <LocationProbe />
    </MemoryRouter>,
  )
}

describe("dashboard routing", () => {
  beforeEach(() => {
    useDaemonMock.mockReturnValue({
      snapshot: makeSnapshot({ revision: 12 }),
      connection: "live",
      loading: false,
      error: null,
      rescan: vi.fn().mockResolvedValue(undefined),
    })
    document.title = "RepoScout"
  })

  it("opens a bookmarked metric and updates the URL when tabs change", async () => {
    const user = userEvent.setup()
    renderRoute("/risk")

    expect(screen.getByText("Highest-risk source files")).toBeInTheDocument()
    expect(screen.getByLabelText("Current route")).toHaveTextContent("/risk")
    expect(document.title).toBe("Risk · RepoScout")

    await user.click(screen.getByRole("tab", { name: "Files" }))

    expect(screen.getByText("Repository files")).toBeInTheDocument()
    expect(screen.getByLabelText("Current route")).toHaveTextContent("/files")
    expect(document.title).toBe("Files · RepoScout")
  })

  it("loads Graph directly from its route", async () => {
    renderRoute("/graph/file/src/api.ts?view=full&direction=dependencies")

    expect(await screen.findByText("Graph for revision 12")).toBeInTheDocument()
    expect(screen.getByLabelText("Current route")).toHaveTextContent(
      "/graph/file/src/api.ts?view=full&direction=dependencies",
    )
    expect(document.title).toBe("Graph · RepoScout")
  })

  it("replaces unknown routes with the overview", async () => {
    renderRoute("/not-a-dashboard-view")

    expect(await screen.findByText("Languages")).toBeInTheDocument()
    expect(screen.getByLabelText("Current route")).toHaveTextContent("/overview")
  })
})
