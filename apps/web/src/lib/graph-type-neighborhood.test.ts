import { describe, expect, it } from "vitest"

import { projectTypeNeighborhood } from "@/lib/graph-type-neighborhood"
import type { DependencyGraph, GraphEdge, GraphSymbol } from "@/lib/types"

function fixture(): DependencyGraph {
  const focus = "src/HttpClient.php"
  const children = Array.from({ length: 8 }, (_, index) => `src/Client${index}.php`)
  const edge_list: GraphEdge[] = [
    ...children.map((path) => ({ source: path, target: focus, resolver: "composer-psr-4" })),
    { source: "src/Factory.php", target: focus, resolver: "composer-psr-4" },
    { source: focus, target: "src/Response.php", resolver: "composer-psr-4" },
    { source: "src/SecondHop.php", target: "src/Factory.php", resolver: "composer-psr-4" },
  ]
  const symbols: GraphSymbol[] = [
    {
      id: "http-client",
      name: "HttpClient",
      qualified_name: "App\\HttpClient",
      kind: "class",
      path: focus,
      language: "PHP",
      line: 4,
      fan_in: children.length,
      fan_out: 0,
    },
    ...children.map((path, index) => ({
      id: `client-${index}`,
      name: `Client${index}`,
      qualified_name: `App\\Client${index}`,
      kind: "class",
      path,
      language: "PHP",
      line: 3,
      fan_in: 0,
      fan_out: 1,
    })),
  ]
  const paths = [focus, ...children, "src/Factory.php", "src/Response.php", "src/SecondHop.php"]
  return {
    languages: ["PHP"],
    nodes: paths.length,
    edges: edge_list.length,
    files: paths.map((path) => ({
      path,
      language: "PHP",
      fan_in: edge_list.filter((edge) => edge.target === path).length,
      fan_out: edge_list.filter((edge) => edge.source === path).length,
      symbol_reach: path === focus
        ? {
            symbol_id: "http-client",
            name: "HttpClient",
            kind: "class",
            fan_in: children.length,
            fan_out: 0,
            relation: "extends",
          }
        : undefined,
    })),
    edge_list,
    symbols,
    symbol_edges: children.map((_, index) => ({
      source: `client-${index}`,
      target: "http-client",
      relation: "extends",
      resolver: "qualified",
    })),
    cycles: [],
    orphans: [],
    top_depended: [],
    most_dependent: [],
    unresolved_imports: 0,
  }
}

describe("type neighborhood projection", () => {
  it("separates declared type reach from direct import context and drops second-hop noise", () => {
    const projection = projectTypeNeighborhood(fixture(), "src/HttpClient.php")!

    expect(projection.symbol.name).toBe("HttpClient")
    expect(projection.groups.map((group) => [group.label, group.name, group.paths.length])).toEqual([
      ["Extends", "HttpClient", 8],
      ["Import dependents", "HttpClient", 1],
      ["Import dependencies", "HttpClient", 1],
    ])
    expect(projection.files).not.toContain("src/SecondHop.php")
    expect(projection.edges).toHaveLength(10)
    expect(projection.edges.filter((edge) => edge.resolver === "symbol-extends")).toHaveLength(8)
    expect(projection.totalFiles).toBe(11)
    expect(projection.truncated).toBe(false)
  })

  it("returns no semantic view when the selected file has no explicit type relationships", () => {
    expect(projectTypeNeighborhood(fixture(), "src/Response.php")).toBeNull()
  })

  it("ignores unsupported future symbol-edge kinds instead of presenting them as inheritance", () => {
    const graph = fixture()
    graph.symbol_edges = graph.symbol_edges!.map((edge) => ({ ...edge, relation: "aliases" }))

    expect(projectTypeNeighborhood(graph, "src/HttpClient.php")).toBeNull()
  })

  it("places declared bases and contracts on the outgoing side", () => {
    const graph = fixture()
    graph.files.push({
      path: "src/ClientContract.php",
      language: "PHP",
      fan_in: 1,
      fan_out: 0,
    })
    graph.symbols!.push({
      id: "client-contract",
      name: "ClientContract",
      qualified_name: "App\\ClientContract",
      kind: "interface",
      path: "src/ClientContract.php",
      language: "PHP",
      line: 3,
      fan_in: 1,
      fan_out: 0,
    })
    graph.symbol_edges!.push({
      source: "http-client",
      target: "client-contract",
      relation: "implements",
      resolver: "qualified",
    })

    const projection = projectTypeNeighborhood(graph, "src/HttpClient.php")!
    const contracts = projection.groups.find((group) => group.label === "Implemented contracts")!

    expect(contracts).toMatchObject({
      direction: "outgoing",
      family: "type",
      relation: "implements",
      paths: ["src/ClientContract.php"],
    })
    expect(projection.edges).toContainEqual({
      source: "src/HttpClient.php",
      target: "src/ClientContract.php",
      resolver: "symbol-implements",
    })
  })

  it("counts relationship containers inside the hard render limit", () => {
    const projection = projectTypeNeighborhood(fixture(), "src/HttpClient.php", 7)!

    expect(projection.files.length + projection.groups.length).toBeLessThanOrEqual(7)
    expect(projection.truncated).toBe(true)
    expect(projection.groups[0]).toMatchObject({ family: "type", label: "Extends" })
  })

  it("caps quiet import context without hiding the real group size", () => {
    const graph = fixture()
    const importers = Array.from({ length: 20 }, (_, index) => `src/Importer${index}.php`)
    graph.files.push(...importers.map((path) => ({
      path,
      language: "PHP",
      fan_in: 0,
      fan_out: 1,
    })))
    graph.edge_list.push(...importers.map((path) => ({
      source: path,
      target: "src/HttpClient.php",
      resolver: "composer-psr-4",
    })))

    const projection = projectTypeNeighborhood(graph, "src/HttpClient.php")!
    const dependents = projection.groups.find((group) => group.label === "Import dependents")!

    expect(dependents.paths).toHaveLength(12)
    expect(dependents.totalMembers).toBe(21)
    expect(projection.totalFiles).toBe(31)
    expect(projection.truncated).toBe(true)
  })
})
