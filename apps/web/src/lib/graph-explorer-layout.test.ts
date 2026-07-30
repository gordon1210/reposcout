import { describe, expect, it } from "vitest"

import { layoutExplorerView } from "@/lib/graph-explorer-layout"
import type {
  ExplorerConnection,
  ExplorerFileSummary,
  ExplorerView,
} from "@/lib/graph-explorer-model"
import { makeFile } from "@/test/fixtures"

function file(path: string, fanIn = 0, fanOut = 0): ExplorerFileSummary {
  return {
    kind: "file",
    id: `file:${path}`,
    path,
    name: path.split("/").at(-1)!,
    parentPath: path.split("/").slice(0, -1).join("/"),
    category: "source",
    external: false,
    report: { ...makeFile(path), language: "TypeScript" },
    graphFile: { path, language: "TypeScript", fan_in: fanIn, fan_out: fanOut },
  }
}

function view(
  entities: ExplorerFileSummary[],
  connections: ExplorerConnection[] = []
): ExplorerView {
  return {
    presentation: "neighborhood",
    focusPath: null,
    scope: {
      kind: "scope",
      id: "scope:.",
      path: "",
      name: "Project",
      scopeKind: "project",
      external: false,
      files: entities.length,
      graphFiles: entities.length,
      tokens: 0,
      sloc: 0,
      findings: 0,
      riskFiles: 0,
      maxCyclomatic: 0,
      minMaintainability: null,
      fanIn: 0,
      fanOut: 0,
      languages: [{ name: "TypeScript", files: entities.length }],
    },
    breadcrumbs: [{ path: "", label: "Project" }],
    entities,
    connections,
    totalEntities: entities.length,
    truncated: false,
  }
}

describe("graph explorer layout", () => {
  it("lays out meaningful file scopes as parent groups with tiered dimensions", () => {
    const entities = [
      file("src/controllers/a.ts", 0, 1),
      file("src/controllers/b.ts", 0, 1),
      file("src/services/a.ts", 2, 1),
      file("src/services/b.ts", 1, 0),
    ]
    const ids = new Map(entities.map((entity) => [entity.path, entity.id]))
    const edges = [
      [entities[0].path, entities[2].path],
      [entities[1].path, entities[2].path],
      [entities[2].path, entities[3].path],
    ]
    const connections = edges.map(([source, target]) => ({
      id: `${ids.get(source)}→${ids.get(target)}`,
      source: ids.get(source)!,
      target: ids.get(target)!,
      count: 1,
      relation: "imports" as const,
      resolvers: [{ resolver: "relative", connections: 1 }],
      fileEdges: [{ source, target, resolver: "relative" }],
    }))

    const layout = layoutExplorerView(
      view(entities, connections),
      "neighborhood"
    )

    expect(layout.groups.map((group) => group.group.path)).toEqual([
      "src/controllers",
      "src/services",
    ])
    expect(layout.entities.get("file:src/controllers/a.ts")?.parentId).toBe(
      "group:src/controllers"
    )
    expect(layout.entities.get("file:src/services/a.ts")).toMatchObject({
      parentId: "group:src/services",
      width: 270,
      height: 104,
    })
    expect(layout.prominence.get("file:src/services/a.ts")?.level).toBe(
      "notable"
    )
  })

  it("keeps group containers inside the hard 100-node render budget", () => {
    const entities = Array.from({ length: 99 }, (_, index) =>
      file(`${index < 50 ? "src/a" : "src/b"}/file-${index}.ts`)
    )

    const layout = layoutExplorerView(view(entities), "neighborhood")

    expect(layout.groups).toEqual([])
    expect(layout.entities.size).toBe(99)
  })

  it("centers a dominant type between explicit and import relationship groups", () => {
    const focus = file("src/HttpClient.php")
    focus.graphFile!.symbol_reach = {
      symbol_id: "http-client",
      name: "HttpClient",
      kind: "class",
      fan_in: 8,
      fan_out: 0,
      relation: "extends",
    }
    const child = file("src/GuzzleClient.php")
    const dependency = file("src/Response.php")
    const semantic: ExplorerView = {
      ...view([focus, child, dependency]),
      presentation: "type",
      focusPath: focus.path,
      groups: [
        {
          id: "relationship:incoming",
          path: "relationship:incoming",
          name: "HttpClient",
          kind: "relationship",
          label: "Extends",
          members: [child],
          languages: [{ name: "TypeScript", files: 1 }],
          relationship: {
            family: "type",
            direction: "incoming",
            relation: "extends",
            description: "Declared subclasses.",
            focusPath: focus.path,
          },
        },
        {
          id: "relationship:outgoing",
          path: "relationship:outgoing",
          name: "HttpClient",
          kind: "relationship",
          label: "Import dependencies",
          members: [dependency],
          languages: [{ name: "TypeScript", files: 1 }],
          relationship: {
            family: "import",
            direction: "outgoing",
            relation: "imports",
            description: "Direct imports.",
            focusPath: focus.path,
          },
        },
      ],
    }

    const layout = layoutExplorerView(semantic, "type")
    const focusPlacement = layout.entities.get(focus.id)!
    const incoming = layout.groups.find(
      (group) => group.group.id === "relationship:incoming"
    )!
    const outgoing = layout.groups.find(
      (group) => group.group.id === "relationship:outgoing"
    )!

    expect(focusPlacement).toMatchObject({ width: 480, height: 176 })
    expect(focusPlacement).not.toHaveProperty("parentId")
    expect(layout.entities.get(child.id)?.parentId).toBe(
      "relationship:incoming"
    )
    expect(layout.entities.get(dependency.id)?.parentId).toBe(
      "relationship:outgoing"
    )
    expect(incoming.position.x + incoming.width).toBeLessThan(
      focusPlacement.position.x
    )
    expect(outgoing.position.x).toBeGreaterThan(
      focusPlacement.position.x + focusPlacement.width
    )
    expect(layout.dense).toBe(false)
  })
})
