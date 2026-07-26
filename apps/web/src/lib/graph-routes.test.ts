import { describe, expect, it } from "vitest"

import {
  GRAPH_ROOT_ROUTE,
  graphRoutePath,
  parseGraphRoute,
  type GraphRoute,
} from "@/lib/graph-routes"

describe("graph routes", () => {
  it("round-trips the architecture root and readable repository scopes", () => {
    expect(parseGraphRoute("/graph")).toEqual(GRAPH_ROOT_ROUTE)
    expect(parseGraphRoute("/graph/")).toEqual(GRAPH_ROOT_ROUTE)

    const route: GraphRoute = {
      kind: "architecture",
      scopePath: "packages/client docs/src",
    }
    const path = graphRoutePath(route)

    expect(path).toBe("/graph/scope/packages/client%20docs/src")
    expect(parseGraphRoute(path)).toEqual(route)
  })

  it("keeps default file neighborhoods compact and serializes non-default controls canonically", () => {
    const defaults: GraphRoute = {
      kind: "file",
      focus: "src/HttpClient.php",
      direction: "both",
      depth: 2,
      presentation: "auto",
    }
    expect(graphRoutePath(defaults)).toBe("/graph/file/src/HttpClient.php")
    expect(parseGraphRoute(graphRoutePath(defaults))).toEqual(defaults)

    const configured: GraphRoute = {
      ...defaults,
      direction: "dependents",
      depth: 3,
      presentation: "full",
    }
    const path = graphRoutePath(configured)

    expect(path).toBe("/graph/file/src/HttpClient.php?view=full&direction=dependents&depth=3")
    expect(parseGraphRoute(path.split("?")[0], `?${path.split("?")[1]}`)).toEqual(configured)
  })

  it("preserves literal percent sequences and rejects malformed or unsafe graph paths", () => {
    const route: GraphRoute = {
      kind: "file",
      focus: "src/Client%2FAdapter #1.ts",
      direction: "dependencies",
      depth: 1,
      presentation: "type",
    }
    const path = graphRoutePath(route)
    const [pathname, search] = path.split("?")

    expect(parseGraphRoute(pathname, `?${search}`)).toEqual(route)
    expect(parseGraphRoute("/graph/file/src/%E0%A4%A")).toBeNull()
    expect(parseGraphRoute("/graph/file/src/%2Fetc")).toBeNull()
    expect(parseGraphRoute("/graph/unknown/src")).toBeNull()
  })

  it("falls back to safe control defaults for unsupported query values", () => {
    expect(parseGraphRoute(
      "/graph/file/src/app.ts",
      "?view=magic&direction=sideways&depth=99",
    )).toEqual({
      kind: "file",
      focus: "src/app.ts",
      direction: "both",
      depth: 2,
      presentation: "auto",
    })
  })
})
