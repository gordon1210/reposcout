import { describe, expect, it } from "vitest"

import {
  graphProminence,
  inspectGraphFile,
  projectGraph,
  searchGraphFiles,
} from "@/lib/graph-data"
import type { DependencyGraph } from "@/lib/types"

function makeGraph(): DependencyGraph {
  const paths = [
    "src/app.ts",
    "src/api.ts",
    "src/db.ts",
    "src/ui.ts",
    "src/unused.ts",
  ]
  const edge_list = [
    { source: "src/app.ts", target: "src/api.ts", resolver: "relative" },
    { source: "src/app.ts", target: "src/ui.ts", resolver: "tsconfig-paths" },
    { source: "src/api.ts", target: "src/db.ts", resolver: "relative" },
  ]
  return {
    languages: ["TypeScript"],
    nodes: paths.length,
    edges: edge_list.length,
    files: paths.map((path) => ({
      path,
      language: "TypeScript",
      fan_in: edge_list.filter((edge) => edge.target === path).length,
      fan_out: edge_list.filter((edge) => edge.source === path).length,
      dependencies: edge_list
        .filter((edge) => edge.source === path)
        .map((edge) => edge.target),
      dependents: edge_list
        .filter((edge) => edge.target === path)
        .map((edge) => edge.source),
    })),
    edge_list,
    cycles: [],
    orphans: ["src/unused.ts"],
    top_depended: [],
    most_dependent: [],
    unresolved_imports: 0,
  }
}

describe("repository graph projection", () => {
  it("follows dependencies to an exact depth", () => {
    const projection = projectGraph(
      makeGraph(),
      "src/app.ts",
      "dependencies",
      1
    )

    expect(projection.files.map((file) => file.path)).toEqual([
      "src/api.ts",
      "src/app.ts",
      "src/ui.ts",
    ])
    expect(projection.edges).toHaveLength(2)
  })

  it("turns reverse traversal into a bounded blast radius", () => {
    const projection = projectGraph(makeGraph(), "src/db.ts", "dependents", 2)

    expect(projection.files.map((file) => file.path)).toEqual([
      "src/api.ts",
      "src/app.ts",
      "src/db.ts",
    ])
  })

  it("caps overview rendering while retaining the most connected files", () => {
    const projection = projectGraph(makeGraph(), null, "both", 2, 2)

    expect(projection.files.map((file) => file.path)).toEqual([
      "src/api.ts",
      "src/app.ts",
    ])
    expect(projection.truncated).toBe(true)
    expect(projection.totalFiles).toBe(5)
  })

  it("ranks filename matches ahead of parent-path matches", () => {
    const graph = makeGraph()
    graph.files.push({
      path: "api/generated/client.ts",
      language: "TypeScript",
      fan_in: 10,
      fan_out: 0,
    })

    expect(searchGraphFiles(graph, "api").map((file) => file.path)).toEqual([
      "src/api.ts",
      "api/generated/client.ts",
    ])
  })

  it("builds actionable file inspection details from graph relationships", () => {
    const inspection = inspectGraphFile(makeGraph(), "src/app.ts")

    expect(inspection?.roles).toContain("Coordinator")
    expect(inspection?.outgoing.map((edge) => edge.target)).toEqual([
      "src/api.ts",
      "src/ui.ts",
    ])
    expect(inspection?.incoming).toEqual([])
    expect(inspection?.resolverUsage).toEqual([
      { resolver: "relative", connections: 1 },
      { resolver: "tsconfig-paths", connections: 1 },
    ])
  })

  it("distinguishes orphan candidates and unknown paths", () => {
    expect(inspectGraphFile(makeGraph(), "src/unused.ts")?.roles).toEqual([
      "Orphan candidate",
    ])
    expect(inspectGraphFile(makeGraph(), "missing.ts")).toBeNull()
  })

  it("uses resolved reach rather than file size for structural prominence", () => {
    expect(graphProminence({ fan_in: 7, fan_out: 1 })).toMatchObject({
      level: "hub",
      label: "High-impact dependency",
    })
    expect(graphProminence({ fan_in: 1, fan_out: 9 })).toMatchObject({
      level: "hub",
      label: "Broad coordinator",
    })
    expect(graphProminence({ fan_in: 3, fan_out: 1 })).toMatchObject({
      level: "notable",
      label: "Shared dependency",
    })
    expect(
      graphProminence({ fan_in: 6, fan_out: 0, language: "Go" })
    ).toMatchObject({
      level: "hub",
      label: "Package anchor",
    })
    expect(graphProminence({ fan_in: 0, fan_out: 1 }).level).toBe("standard")
  })

  it("uses explicit symbol relationships for traversal and prominence", () => {
    const graph = makeGraph()
    graph.files.find((file) => file.path === "src/db.ts")!.symbol_reach = {
      symbol_id: "db:BaseStore",
      name: "BaseStore",
      kind: "class",
      fan_in: 9,
      fan_out: 0,
      relation: "extends",
    }
    graph.symbols = [
      {
        id: "db:BaseStore",
        name: "BaseStore",
        qualified_name: "BaseStore",
        kind: "class",
        path: "src/db.ts",
        language: "TypeScript",
        line: 1,
        fan_in: 1,
        fan_out: 0,
      },
      {
        id: "unused:CustomStore",
        name: "CustomStore",
        qualified_name: "CustomStore",
        kind: "class",
        path: "src/unused.ts",
        language: "TypeScript",
        line: 1,
        fan_in: 0,
        fan_out: 1,
      },
    ]
    graph.symbol_edges = [
      {
        source: "unused:CustomStore",
        target: "db:BaseStore",
        relation: "extends",
        resolver: "unique-name",
      },
    ]

    expect(
      projectGraph(graph, "src/db.ts", "dependents", 1).files.map(
        (file) => file.path
      )
    ).toEqual(["src/api.ts", "src/db.ts", "src/unused.ts"])
    expect(
      graphProminence(graph.files.find((file) => file.path === "src/db.ts")!)
    ).toMatchObject({
      level: "hub",
      label: "Base class",
      basis: "symbol",
      reach: 9,
    })
    expect(
      inspectGraphFile(graph, "src/db.ts")?.symbolRelations[0]
    ).toMatchObject({
      direction: "incoming",
      relation: "extends",
      symbol: { name: "CustomStore", path: "src/unused.ts" },
    })
  })
})
