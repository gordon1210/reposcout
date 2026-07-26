import type { ReactNode } from "react"
import { render, screen, waitFor, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { MemoryRouter, useLocation, useNavigate } from "react-router"
import { beforeEach, describe, expect, it, vi } from "vitest"

import { RepositoryGraph } from "@/components/repository-graph"
import type { DependencyGraph, ScanReport } from "@/lib/types"
import { makeFile, makeReport } from "@/test/fixtures"

const mocks = vi.hoisted(() => ({
  useRepositoryGraph: vi.fn(),
}))

vi.mock("@/hooks/use-repository-graph", () => ({
  useRepositoryGraph: mocks.useRepositoryGraph,
}))

vi.mock("@xyflow/react", () => ({
  Background: () => null,
  BackgroundVariant: { Dots: "dots" },
  BaseEdge: () => null,
  Controls: () => null,
  EdgeLabelRenderer: ({ children }: { children: ReactNode }) => children,
  Handle: () => null,
  MarkerType: { ArrowClosed: "arrow-closed" },
  Panel: ({ children }: { children: ReactNode }) => <div>{children}</div>,
  useStore: (selector: (state: { transform: [number, number, number] }) => unknown) =>
    selector({ transform: [0, 0, 1] }),
  MiniMap: ({ ariaLabel, bgColor, maskColor, nodeColor, style }: {
    ariaLabel: string
    bgColor: string
    maskColor: string
    nodeColor: (node: { data: { entity: { kind: "file"; category: "source"; report: { language: string } } } }) => string
    style: { width: number; height: number }
  }) => (
    <div
      role="img"
      aria-label={ariaLabel}
      data-bg-color={bgColor}
      data-mask-color={maskColor}
      data-node-color={nodeColor({
        data: {
          entity: { kind: "file", category: "source", report: { language: "TypeScript" } },
        },
      })}
      data-width={style.width}
      data-height={style.height}
    />
  ),
  Position: { Bottom: "bottom", Left: "left", Right: "right", Top: "top" },
  ReactFlow: ({ nodes, edges, minZoom, fitViewOptions, onNodeClick, onNodeDoubleClick, onNodeMouseEnter, onNodeMouseLeave, onEdgeClick, children }: {
    nodes: Array<{
      id: string
      type: "explorerEntity" | "explorerGroup"
      width: number
      height: number
      parentId?: string
      selected?: boolean
      data: {
        entity?: { kind: "scope" | "file"; path: string }
        group?: {
          id: string
          path: string
          kind: "architecture" | "path" | "relationship"
          label: string
          name: string
        }
        related: boolean
        dimmed: boolean
        vertical?: boolean
        prominence?: { level: string }
        typeFocused?: boolean
      }
    }>
    edges: Array<{
      id: string
      source: string
      target: string
      data: { connection: { fileEdges: Array<{ source: string; target: string }> } }
      markerEnd?: unknown
      style: { opacity: number }
    }>
    minZoom: number
    fitViewOptions: { minZoom: number }
    onNodeClick?: (event: React.MouseEvent, node: (typeof nodes)[number]) => void
    onNodeDoubleClick?: (event: React.MouseEvent, node: (typeof nodes)[number]) => void
    onNodeMouseEnter?: (event: React.MouseEvent, node: (typeof nodes)[number]) => void
    onNodeMouseLeave?: (event: React.MouseEvent, node: (typeof nodes)[number]) => void
    onEdgeClick?: (event: React.MouseEvent, edge: { id: string; source: string; target: string; data: { connection: { fileEdges: Array<{ source: string; target: string }> } } }) => void
    children: ReactNode
  }) => (
    <div data-testid="react-flow" data-min-zoom={minZoom} data-fit-min-zoom={fitViewOptions.minZoom}>
      {nodes.map((node) => (
        <button
          key={node.id}
          type="button"
          data-related={node.data.related}
          data-dimmed={node.data.dimmed}
          data-node-width={node.width}
          data-node-height={node.height}
          data-vertical={node.data.vertical}
          data-parent-id={node.parentId}
          data-prominence={node.data.prominence?.level}
          data-type-focused={node.data.typeFocused}
          data-selected={node.selected}
          onClick={(event) => onNodeClick?.(event, node)}
          onDoubleClick={(event) => onNodeDoubleClick?.(event, node)}
          onMouseEnter={(event) => onNodeMouseEnter?.(event, node)}
          onMouseLeave={(event) => onNodeMouseLeave?.(event, node)}
        >
          {node.data.group
            ? node.data.group.kind === "relationship"
              ? `Select relationship group ${node.data.group.label} ${node.data.group.name}`
              : `Select group ${node.data.group.path}`
            : `${node.data.entity!.kind === "scope" ? "Open scope" : "Select file"} ${node.data.entity!.path}`}
        </button>
      ))}
      {edges.map((edge) => (
        <button
          key={edge.id}
          type="button"
          data-edge-opacity={edge.style.opacity}
          data-has-marker={Boolean(edge.markerEnd)}
          onClick={(event) => onEdgeClick?.(event, edge)}
        >
          Connection {edge.data.connection.fileEdges[0].source} to {edge.data.connection.fileEdges[0].target}
        </button>
      ))}
      {children}
    </div>
  ),
  getBezierPath: () => ["", 0, 0],
}))

function LocationProbe() {
  const location = useLocation()
  const navigate = useNavigate()
  return (
    <div>
      <output aria-label="Current graph route">{location.pathname}{location.search}</output>
      <button type="button" onClick={() => navigate(-1)}>Browser back</button>
    </div>
  )
}

function renderGraph(revision: number, report: ScanReport, route = "/graph") {
  return render(
    <MemoryRouter initialEntries={[route]}>
      <RepositoryGraph revision={revision} report={report} />
      <LocationProbe />
    </MemoryRouter>,
  )
}

function makeGraph(): DependencyGraph {
  const edge_list = [
    { source: "src/app.ts", target: "src/api.ts", resolver: "tsconfig-paths" },
    { source: "src/api.ts", target: "src/db.ts", resolver: "relative" },
  ]
  return {
    languages: ["TypeScript"],
    nodes: 3,
    edges: 2,
    files: ["src/app.ts", "src/api.ts", "src/db.ts"].map((path) => ({
      path,
      language: "TypeScript",
      fan_in: edge_list.filter((edge) => edge.target === path).length,
      fan_out: edge_list.filter((edge) => edge.source === path).length,
    })),
    edge_list,
    cycles: [],
    orphans: [],
    top_depended: [],
    most_dependent: [],
    unresolved_imports: 1,
  }
}

function makeGraphReport() {
  const report = makeReport()
  report.files = [
    { ...makeFile("src/app.ts", 300), language: "TypeScript" },
    { ...makeFile("src/api.ts", 540), language: "TypeScript", markers: { TODO: 2 } },
    { ...makeFile("src/db.ts", 220), language: "TypeScript" },
  ]
  report.summary.top_risks = [
    {
      path: "src/api.ts",
      score: 0.81,
      sloc: 16,
      cyclomatic: 4,
      churn_commits: 3,
      untested: true,
      reasons: ["high complexity", "no matching test"],
    },
  ]
  report.finding_catalog.findings = [
    {
      fingerprint: "api-marker",
      kind: "marker",
      severity: "warning",
      message: "TODO marker in API",
      primary_location: { path: "src/api.ts", start_line: 12, end_line: 12 },
    },
  ]
  return report
}

function makeDenseGraph() {
  const paths = Array.from({ length: 13 }, (_, index) => `src/module-${index}.ts`)
  const edge_list = [
    ...paths.slice(1).map((path) => ({
      source: paths[0],
      target: path,
      resolver: "relative",
    })),
    ...paths.slice(1, -1).map((path, index) => ({
      source: path,
      target: paths[index + 2],
      resolver: "relative",
    })),
  ]
  const graph: DependencyGraph = {
    languages: ["TypeScript"],
    nodes: paths.length,
    edges: edge_list.length,
    files: paths.map((path) => ({
      path,
      language: "TypeScript",
      fan_in: edge_list.filter((edge) => edge.target === path).length,
      fan_out: edge_list.filter((edge) => edge.source === path).length,
    })),
    edge_list,
    cycles: [],
    orphans: [],
    top_depended: [],
    most_dependent: [],
    unresolved_imports: 0,
  }
  const report = makeReport()
  report.files = paths.map((path) => ({ ...makeFile(path, 100), language: "TypeScript" }))
  return { graph, report }
}

function makeGroupedGraph() {
  const paths = [
    "src/controllers/controller-a.ts",
    "src/controllers/controller-b.ts",
    "src/services/service-a.ts",
    "src/services/service-b.ts",
  ]
  const edge_list = [
    { source: paths[0], target: paths[2], resolver: "relative" },
    { source: paths[1], target: paths[2], resolver: "relative" },
    { source: paths[2], target: paths[3], resolver: "relative" },
  ]
  const graph: DependencyGraph = {
    languages: ["TypeScript"],
    nodes: paths.length,
    edges: edge_list.length,
    files: paths.map((path) => ({
      path,
      language: "TypeScript",
      fan_in: edge_list.filter((edge) => edge.target === path).length,
      fan_out: edge_list.filter((edge) => edge.source === path).length,
    })),
    edge_list,
    cycles: [],
    orphans: [],
    top_depended: [],
    most_dependent: [],
    unresolved_imports: 0,
  }
  const report = makeReport()
  report.files = paths.map((path) => ({ ...makeFile(path, 100), language: "TypeScript" }))
  return { graph, report }
}

beforeEach(() => {
  mocks.useRepositoryGraph.mockReturnValue({
    graph: makeGraph(),
    loading: false,
    error: null,
    retry: vi.fn(),
  })
})

describe("RepositoryGraph", () => {
  it("keeps dense graphs readable and gives the minimap measured, visible nodes", async () => {
    const user = userEvent.setup()
    const { graph, report } = makeDenseGraph()
    mocks.useRepositoryGraph.mockReturnValue({
      graph,
      loading: false,
      error: null,
      retry: vi.fn(),
    })
    renderGraph(6, report)

    expect(screen.getByTestId("react-flow")).toHaveAttribute("data-min-zoom", "0.08")
    expect(screen.getByTestId("react-flow")).toHaveAttribute("data-fit-min-zoom", "0.42")
    const file = screen.getByRole("button", { name: "Select file src/module-0.ts" })
    expect(file).toHaveAttribute("data-node-width", "304")
    expect(file).toHaveAttribute("data-node-height", "116")
    expect(file).toHaveAttribute("data-prominence", "hub")
    expect(file).toHaveAttribute("data-vertical", "false")

    const overview = screen.getByRole("img", { name: "Repository graph overview" })
    expect(overview).toHaveAttribute("data-bg-color", "#0b0f14")
    expect(overview).toHaveAttribute("data-node-color", "#60a5fa")
    expect(overview).toHaveAttribute("data-width", "224")
    expect(overview).toHaveAttribute("data-height", "152")

    const idleEdge = screen.getByRole("button", {
      name: "Connection src/module-0.ts to src/module-1.ts",
    })
    expect(idleEdge).toHaveAttribute("data-edge-opacity", "0.1")
    expect(idleEdge).toHaveAttribute("data-has-marker", "false")

    await user.hover(file)
    expect(screen.getByRole("button", {
      name: "Connection src/module-0.ts to src/module-1.ts",
    })).toHaveAttribute("data-edge-opacity", "0.94")
    expect(screen.getByRole("button", {
      name: "Connection src/module-0.ts to src/module-1.ts",
    })).toHaveAttribute("data-has-marker", "true")

    await user.unhover(file)
    expect(screen.getByRole("button", {
      name: "Connection src/module-0.ts to src/module-1.ts",
    })).toHaveAttribute("data-edge-opacity", "0.1")

    expect(screen.getByText("Legend")).toBeInTheDocument()
    expect(screen.getByText("arrow points at the dependency")).toBeInTheDocument()
    expect(screen.getByText("Hub files")).toBeInTheDocument()
    expect(screen.getByText("Unresolved imports")).toBeInTheDocument()

    await user.click(file)
    expect(screen.getByText("Broad coordinator")).toBeInTheDocument()
    expect(screen.getByText(/coordinates 12 resolved direct dependencies/)).toBeInTheDocument()
    expect(screen.getByText(/ambiguous type relationships are never inferred/)).toBeInTheDocument()
  })

  it("selects direct connections on one click and opens nodes on double-click", async () => {
    const user = userEvent.setup()
    renderGraph(4, makeGraphReport())

    const scope = screen.getByRole("button", { name: "Select group src" })
    await user.click(scope)
    expect(screen.getByRole("button", { name: "Select group src" })).toBeInTheDocument()
    expect(screen.getByRole("heading", { name: "src" })).toBeInTheDocument()

    const app = screen.getByRole("button", { name: "Select file src/app.ts" })
    expect(app).toHaveAttribute("data-parent-id", "architecture-group:src")
    expect(screen.getByRole("combobox", { name: "Graph direction" })).toBeDisabled()

    await user.click(app)
    expect(screen.getByRole("button", { name: "Select file src/app.ts" })).toHaveAttribute("data-related", "true")
    expect(screen.getByRole("button", { name: "Select file src/api.ts" })).toHaveAttribute("data-related", "true")
    expect(screen.getByRole("button", { name: "Select file src/db.ts" })).toHaveAttribute("data-dimmed", "true")
    expect(screen.getByRole("combobox", { name: "Graph direction" })).toBeDisabled()

    await user.dblClick(screen.getByRole("button", { name: "Select file src/app.ts" }))
    expect(screen.getByRole("combobox", { name: "Graph direction" })).toBeEnabled()
    expect(screen.getByLabelText("Current graph route").textContent).toBe(
      "/graph/file/src/app.ts",
    )

    await user.click(screen.getByRole("button", { name: "Browser back" }))
    expect(screen.getByLabelText("Current graph route").textContent).toBe("/graph")
    expect(screen.getByRole("combobox", { name: "Graph direction" })).toBeDisabled()
  })

  it("groups focused files by path scope and highlights group boundaries factually", async () => {
    const user = userEvent.setup()
    const { graph, report } = makeGroupedGraph()
    mocks.useRepositoryGraph.mockReturnValue({
      graph,
      loading: false,
      error: null,
      retry: vi.fn(),
    })
    renderGraph(7, report)

    await user.type(
      screen.getByRole("combobox", { name: "Find a file in the repository graph" }),
      "controller-a",
    )
    await user.click(screen.getByRole("option", { name: /controller-a\.ts/ }))
    await user.dblClick(screen.getByRole("button", {
      name: "Select file src/controllers/controller-a.ts",
    }))

    const controllers = screen.getByRole("button", { name: "Select group src/controllers" })
    expect(controllers).toBeInTheDocument()
    expect(screen.getByRole("button", { name: "Select group src/services" })).toBeInTheDocument()
    expect(screen.getByRole("button", {
      name: "Select file src/controllers/controller-a.ts",
    })).toHaveAttribute("data-parent-id", "group:src/controllers")
    expect(screen.getByRole("button", {
      name: "Select file src/services/service-a.ts",
    })).toHaveAttribute("data-prominence", "notable")

    await user.click(controllers)

    expect(screen.getByRole("heading", { name: "controllers" })).toBeInTheDocument()
    expect(screen.getByRole("button", {
      name: "Select file src/services/service-a.ts",
    })).toHaveAttribute("data-related", "true")
    expect(screen.getByRole("button", {
      name: "Select file src/services/service-b.ts",
    })).toHaveAttribute("data-dimmed", "true")

    await user.dblClick(screen.getByRole("button", { name: "Select group src/controllers" }))
    expect(screen.queryByRole("button", { name: "Select group src/controllers" })).not.toBeInTheDocument()
    expect(screen.getByRole("button", {
      name: "Select file src/controllers/controller-b.ts",
    })).toBeInTheDocument()
  })

  it("makes explicitly extended base classes visibly dominant without extra drill-down", async () => {
    const user = userEvent.setup()
    const graph = makeGraph()
    graph.files.find((file) => file.path === "src/api.ts")!.symbol_reach = {
      symbol_id: "api:HttpClient",
      name: "HttpClient",
      kind: "class",
      fan_in: 27,
      fan_out: 0,
      relation: "extends",
    }
    graph.symbols = [
      {
        id: "api:HttpClient",
        name: "HttpClient",
        qualified_name: "App\\HttpClient",
        kind: "class",
        path: "src/api.ts",
        language: "TypeScript",
        line: 4,
        fan_in: 1,
        fan_out: 0,
      },
      {
        id: "app:GuzzleClient",
        name: "GuzzleClient",
        qualified_name: "App\\GuzzleClient",
        kind: "class",
        path: "src/app.ts",
        language: "TypeScript",
        line: 8,
        fan_in: 0,
        fan_out: 1,
      },
    ]
    graph.symbol_edges = [{
      source: "app:GuzzleClient",
      target: "api:HttpClient",
      relation: "extends",
      resolver: "qualified",
    }]
    mocks.useRepositoryGraph.mockReturnValue({ graph, loading: false, error: null, retry: vi.fn() })
    renderGraph(8, makeGraphReport())

    expect(screen.getByRole("button", { name: "Select group src" })).toBeInTheDocument()
    const base = screen.getByRole("button", { name: "Select file src/api.ts" })
    expect(base).toHaveAttribute("data-parent-id", "architecture-group:src")
    expect(base).toHaveAttribute("data-node-width", "414")
    expect(base).toHaveAttribute("data-node-height", "154")
    expect(base).toHaveAttribute("data-prominence", "hub")

    await user.click(base)
    expect(screen.getByText("Base class")).toBeInTheDocument()
    expect(screen.getByText(/27 declared types directly extend HttpClient/)).toBeInTheDocument()
    expect(screen.getByText(/explicit extends, implements, or embeds syntax/)).toBeInTheDocument()

    await user.click(screen.getByRole("tab", { name: "Relations" }))
    expect(screen.getByText("Explicit type relationships")).toBeInTheDocument()
    expect(screen.getByText("App\\GuzzleClient")).toBeInTheDocument()

    await user.dblClick(base)

    expect(screen.getByLabelText("Current graph route").textContent).toBe(
      "/graph/file/src/api.ts",
    )

    const semanticBase = screen.getByRole("button", { name: "Select file src/api.ts" })
    expect(semanticBase).toHaveAttribute("data-node-width", "480")
    expect(semanticBase).toHaveAttribute("data-node-height", "176")
    expect(semanticBase).toHaveAttribute("data-type-focused", "true")
    expect(screen.getByRole("button", { name: "Select file src/app.ts" })).toHaveAttribute(
      "data-parent-id",
      "relationship:src/api.ts:type:incoming:extends",
    )
    expect(screen.getByRole("combobox", { name: "Graph direction" })).toBeDisabled()
    expect(screen.getByText("Type structure · direct declared relationships")).toBeInTheDocument()

    const extenders = screen.getByRole("button", {
      name: "Select relationship group Extends HttpClient",
    })
    await user.click(extenders)
    expect(screen.getByRole("heading", { name: "Extends HttpClient" })).toBeInTheDocument()
    expect(screen.getByText(/explicitly extend App\\HttpClient/)).toBeInTheDocument()

    await user.click(screen.getByRole("button", { name: "Show full neighborhood" }))
    expect(screen.queryByRole("button", {
      name: "Select relationship group Extends HttpClient",
    })).not.toBeInTheDocument()
    expect(screen.getByRole("combobox", { name: "Graph direction" })).toBeEnabled()
    expect(screen.getByLabelText("Current graph route").textContent).toBe(
      "/graph/file/src/api.ts?view=full",
    )

    const breadcrumbs = screen.getByRole("navigation", { name: "Graph location" })
    await user.click(within(breadcrumbs).getByRole("button", { name: "src" }))
    expect(screen.getByLabelText("Current graph route").textContent).toBe("/graph/scope/src")
    expect(screen.getByRole("navigation", { name: "Graph location" })).toBeInTheDocument()
  })

  it("restores a bookmarked file neighborhood and its routed controls", () => {
    renderGraph(
      9,
      makeGraphReport(),
      "/graph/file/src/api.ts?view=full&direction=dependencies&depth=1",
    )

    expect(screen.getByRole("combobox", { name: "Graph direction" })).toHaveValue("dependencies")
    expect(screen.getByRole("combobox", { name: "Graph depth" })).toHaveValue("1")
    const breadcrumbs = screen.getByRole("navigation", { name: "Graph location" })
    expect(within(breadcrumbs).getByText("src/api.ts")).toBeInTheDocument()
    expect(screen.getByLabelText("Current graph route").textContent).toBe(
      "/graph/file/src/api.ts?view=full&direction=dependencies&depth=1",
    )
  })

  it("replaces malformed and stale graph locations with a safe canonical route", async () => {
    const { unmount } = renderGraph(
      10,
      makeGraphReport(),
      "/graph/file/missing.ts?view=magic&depth=99",
    )

    await waitFor(() => {
      expect(screen.getByLabelText("Current graph route").textContent).toBe("/graph")
    })

    unmount()
    renderGraph(10, makeGraphReport(), "/graph/scope/stale/path")
    await waitFor(() => {
      expect(screen.getByLabelText("Current graph route").textContent).toBe("/graph")
    })
  })

  it("expands inside the app and turns a selected file into an actionable inspection", async () => {
    const user = userEvent.setup()
    renderGraph(4, makeGraphReport())

    await user.type(screen.getByRole("combobox", { name: "Find a file in the repository graph" }), "api")
    await user.click(screen.getByRole("option", { name: /src\/api\.ts/ }))

    expect(screen.getByRole("heading", { name: "src/api.ts" })).toBeInTheDocument()
    expect(screen.getByText("Latest scan")).toBeInTheDocument()
    expect(screen.getByText("Risk 0.81")).toBeInTheDocument()
    expect(screen.getByText("TODO marker in API")).toBeInTheDocument()
    expect(screen.getByRole("button", { name: "Dependencies" })).toBeInTheDocument()

    await user.click(screen.getByRole("button", { name: "Expand graph workspace" }))
    expect(screen.getByRole("button", { name: "Restore graph workspace" })).toBeInTheDocument()
    expect(document.body.style.overflow).toBe("hidden")

    await user.keyboard("{Escape}")
    expect(screen.getByRole("button", { name: "Expand graph workspace" })).toBeInTheDocument()
    expect(document.body.style.overflow).toBe("")
  })

  it("inspects a selected connection and explains resolver provenance", async () => {
    const user = userEvent.setup()
    renderGraph(4, makeGraphReport())

    await user.click(screen.getByRole("button", { name: "Connection src/app.ts to src/api.ts" }))

    expect(screen.getByRole("heading", { name: "Connection details" })).toBeInTheDocument()
    expect(screen.getAllByText("tsconfig Paths")).not.toHaveLength(0)
    expect(screen.getByText(/paths mapping resolved this alias/)).toBeInTheDocument()
    expect(screen.getByText("1 file connection")).toBeInTheDocument()
  })

  it("explains Composer provenance for PHP graph connections", async () => {
    const user = userEvent.setup()
    mocks.useRepositoryGraph.mockReturnValue({
      graph: {
        languages: ["PHP"],
        nodes: 2,
        edges: 1,
        files: [
          { path: "src/Controller.php", language: "PHP", fan_in: 0, fan_out: 1 },
          { path: "src/Service.php", language: "PHP", fan_in: 1, fan_out: 0 },
        ],
        edge_list: [
          { source: "src/Controller.php", target: "src/Service.php", resolver: "composer-psr-4" },
        ],
        cycles: [],
        orphans: [],
        top_depended: [],
        most_dependent: [],
        unresolved_imports: 0,
        config_files: ["composer.json"],
      },
      loading: false,
      error: null,
      retry: vi.fn(),
    })

    const report = makeGraphReport()
    report.files = [
      { ...makeFile("src/Controller.php", 300), language: "PHP" },
      { ...makeFile("src/Service.php", 240), language: "PHP" },
    ]
    renderGraph(5, report)
    await user.click(screen.getByRole("button", { name: "Connection src/Controller.php to src/Service.php" }))

    expect(screen.getAllByText("Composer PSR 4")).not.toHaveLength(0)
    expect(screen.getByText(/Composer PSR-4 autoload mapping/)).toBeInTheDocument()
    expect(screen.getByText("composer.json")).toBeInTheDocument()
  })
})
