export function normalizeScope(path: string): string {
  return path.replaceAll("\\", "/").replace(/^\/+|\/+$/g, "")
}

export function relativeToScope(path: string, scope: string): string {
  if (!scope) return path
  return path.slice(scope.length + 1)
}

export function pathInScope(path: string, scope: string): boolean {
  return !scope || path === scope || path.startsWith(`${scope}/`)
}

export function pathParent(path: string): string {
  const parts = path.split("/")
  parts.pop()
  return parts.join("/")
}

export function joinPath(parent: string, child: string): string {
  return parent ? `${parent}/${child}` : child
}

export function scopeId(path: string, external: boolean): string {
  return `${external ? "external-scope" : "scope"}:${path || "."}`
}

export function fileId(path: string, external: boolean): string {
  return `${external ? "external-file" : "file"}:${path}`
}
