import type { GraphDirection } from "@/lib/graph-data"
import type { ExplorerNeighborhoodPresentation } from "@/lib/graph-explorer-model"

export type GraphRouteDepth = 1 | 2 | 3

export type GraphRoute =
  | {
      kind: "architecture"
      scopePath: string
    }
  | {
      kind: "file"
      focus: string
      direction: GraphDirection
      depth: GraphRouteDepth
      presentation: ExplorerNeighborhoodPresentation
    }

export const GRAPH_ROOT_ROUTE: GraphRoute = {
  kind: "architecture",
  scopePath: "",
}

const GRAPH_PATH = "/graph"
const SCOPE_PATH = `${GRAPH_PATH}/scope`
const FILE_PATH = `${GRAPH_PATH}/file`

export function parseGraphRoute(
  pathname: string,
  search = ""
): GraphRoute | null {
  const path = stripTrailingSlashes(pathname)
  if (path === GRAPH_PATH) return GRAPH_ROOT_ROUTE

  if (path === SCOPE_PATH) return GRAPH_ROOT_ROUTE
  if (path.startsWith(`${SCOPE_PATH}/`)) {
    const scopePath = decodeRepositoryPath(path.slice(SCOPE_PATH.length + 1))
    return scopePath === null ? null : { kind: "architecture", scopePath }
  }

  if (!path.startsWith(`${FILE_PATH}/`)) return null
  const focus = decodeRepositoryPath(path.slice(FILE_PATH.length + 1))
  if (!focus) return null

  const params = new URLSearchParams(search)
  return {
    kind: "file",
    focus,
    direction: parseDirection(params.get("direction")),
    depth: parseGraphRouteDepth(params.get("depth")),
    presentation: parsePresentation(params.get("view")),
  }
}

export function graphRoutePath(route: GraphRoute): string {
  if (route.kind === "architecture") {
    return route.scopePath
      ? `${SCOPE_PATH}/${encodeRepositoryPath(route.scopePath)}`
      : GRAPH_PATH
  }

  const params = new URLSearchParams()
  if (route.presentation !== "auto") params.set("view", route.presentation)
  if (route.direction !== "both") params.set("direction", route.direction)
  if (route.depth !== 2) params.set("depth", String(route.depth))
  const query = params.toString()
  const path = `${FILE_PATH}/${encodeRepositoryPath(route.focus)}`
  return query ? `${path}?${query}` : path
}

function encodeRepositoryPath(path: string): string {
  return path
    .replace(/^\/+|\/+$/g, "")
    .split("/")
    .map((segment) => encodeURIComponent(segment))
    .join("/")
}

function decodeRepositoryPath(path: string): string | null {
  if (!path) return null
  try {
    const segments = path
      .split("/")
      .map((segment) => decodeURIComponent(segment))
    const invalid = segments.some(
      (segment) =>
        !segment ||
        segment === "." ||
        segment === ".." ||
        segment.includes("/") ||
        segment.includes("\0")
    )
    if (invalid) return null
    return segments.join("/")
  } catch {
    return null
  }
}

function stripTrailingSlashes(path: string): string {
  const stripped = path.replace(/\/+$/g, "")
  return stripped || "/"
}

function parseDirection(value: string | null): GraphDirection {
  if (value === "dependencies" || value === "dependents") return value
  return "both"
}

export function parseGraphRouteDepth(value: string | null): GraphRouteDepth {
  if (value === "1") return 1
  if (value === "3") return 3
  return 2
}

function parsePresentation(
  value: string | null
): ExplorerNeighborhoodPresentation {
  if (value === "full" || value === "type") return value
  return "auto"
}
