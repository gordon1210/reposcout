import type {
  ExplorerConnection,
  ExplorerEntity,
  ExplorerFileSummary,
} from "@/lib/graph-explorer-model"
import type { GraphProminence } from "@/lib/graph-data"
import type { GraphEdge } from "@/lib/types"

export function entityAriaLabel(
  entity: ExplorerEntity,
  prominence: GraphProminence
): string {
  if (entity.kind === "scope") {
    return `${entity.path || "Project"}, ${entity.files} files, ${entity.graphFiles} topology files, ${entity.fanIn} incoming and ${entity.fanOut} outgoing relationships`
  }
  const graph = entity.graphFile
  const reach =
    prominence.level === "standard"
      ? ""
      : `, ${prominence.label}: ${prominence.reason}`
  const relationships = graph
    ? `, ${graph.fan_in} incoming and ${graph.fan_out} outgoing relationships`
    : ""
  return `${entity.path}, ${entity.report.language}, ${categoryLabel(entity.category)}${relationships}${reach}`
}

export function relationLabel(
  relation: ExplorerConnection["relation"]
): string {
  const labels: Record<ExplorerConnection["relation"], string> = {
    imports: "imports",
    includes: "includes",
    "declares-module": "declares module",
    "imports-package": "imports package",
    extends: "extends",
    implements: "implements",
    embeds: "embeds",
    mixed: "mixed relationships",
  }
  return labels[relation]
}

export function resolverLabel(resolver: string): string {
  return resolver
    .split("-")
    .map((part) => resolverLabelPart(part))
    .join(" ")
}

function resolverLabelPart(part: string): string {
  const labels: Record<string, string> = {
    tsconfig: "tsconfig",
    psr: "PSR",
    php: "PHP",
    go: "Go",
    rust: "Rust",
  }
  return labels[part] ?? part.charAt(0).toUpperCase() + part.slice(1)
}

export function resolverDescription(resolver: string): string {
  const descriptions: Record<string, string> = {
    relative:
      "A relative JavaScript or TypeScript specifier resolved against the importing file.",
    "tsconfig-paths":
      "A tsconfig or jsconfig paths mapping resolved this alias.",
    "tsconfig-base-url":
      "A tsconfig or jsconfig baseUrl resolved this non-relative import.",
    "heuristic-alias":
      "RepoScout's conventional @/ alias fallback resolved this import.",
    "package-imports":
      "The importing package's package.json imports map resolved this private alias.",
    "package-exports":
      "A local package.json exports map resolved this public package path.",
    "package-subpath":
      "A local workspace package subpath resolved directly to this source file.",
    "package-entrypoint":
      "A local workspace package entrypoint resolved to this file.",
    "package-index":
      "A local workspace package directory resolved through its index file.",
    "python-relative":
      "A dotted relative Python import resolved within the current package.",
    "python-absolute":
      "An unambiguous repository-absolute Python module path resolved to this file.",
    "python-src-root":
      "A conventional Python src root resolved this absolute module path.",
    "composer-psr-4":
      "A Composer PSR-4 autoload mapping resolved this PHP namespace.",
    "composer-psr-0":
      "A Composer PSR-0 autoload mapping resolved this legacy PHP class name.",
    "php-include":
      "A static PHP include or require expression resolved to this file.",
    "php-namespace-heuristic":
      "A conventional PHP src, app, or lib namespace layout resolved this target.",
    "rust-mod": "A Rust mod declaration resolved to its module source file.",
    "rust-path":
      "A Rust #[path] module attribute resolved to its explicit source file.",
    "rust-use":
      "A crate, self, super, or unambiguous local Rust use path resolved to a module file.",
    "rust-workspace":
      "A local Cargo package or library crate name resolved this Rust use path.",
    "go-module":
      "A local go.mod module prefix resolved the imported Go package; its stable representative file anchors the package edge.",
    "go-relative":
      "A relative Go package path resolved to its stable representative file.",
    "symbol-extends":
      "An explicit class, interface, or trait base resolved to an unambiguous repository symbol.",
    "symbol-implements":
      "An explicit interface or trait implementation resolved to an unambiguous repository symbol.",
    "symbol-embeds":
      "An explicit Go interface or struct embedding resolved to an unambiguous repository symbol.",
  }
  return (
    descriptions[resolver] ??
    `RepoScout resolved this relationship using the ${resolverLabel(resolver)} strategy.`
  )
}

export function graphEdgeId(edge: GraphEdge): string {
  return `${edge.source}→${edge.target}:${edge.resolver}`
}

export function categoryLabel(
  category: ExplorerFileSummary["category"]
): string {
  const labels: Record<ExplorerFileSummary["category"], string> = {
    source: "Source",
    test: "Test",
    config: "Config",
    schema: "Schema",
    entrypoint: "Entrypoint",
    generated: "Generated",
  }
  return labels[category]
}

export function scopeKindLabel(
  kind: Extract<ExplorerEntity, { kind: "scope" }>["scopeKind"]
): string {
  const labels = {
    project: "Project",
    package: "Package",
    area: "Area",
    directory: "Directory",
  }
  return labels[kind]
}

export function shortDate(value?: string): string {
  return value ? value.slice(0, 10) : "?"
}

export function miniMapEntityColor(entity: ExplorerEntity): string {
  if (entity.kind === "scope") return miniMapScopeColor(entity)
  if (entity.category === "generated") return "#64748b"
  const language = entity.report.language.toLowerCase()
  if (language.includes("typescript") || language === "tsx") return "#60a5fa"
  if (language.includes("javascript") || language === "jsx") return "#fde047"
  if (language.includes("python")) return "#38bdf8"
  if (language === "php") return "#c084fc"
  if (language === "rust") return "#fb923c"
  if (language === "go") return "#22d3ee"
  return "#94a3b8"
}

function miniMapScopeColor(
  scope: Extract<ExplorerEntity, { kind: "scope" }>
): string {
  if (scope.external) return "#64748b"
  if (scope.riskFiles > 0) return "#fb7185"
  if (scope.findings > 0) return "#fbbf24"
  return "#2dd4bf"
}

export function scopeColor(
  scope: Extract<ExplorerEntity, { kind: "scope" }>
): string {
  if (scope.external) return "var(--muted-foreground)"
  if (scope.riskFiles > 0) return "var(--chart-5)"
  if (scope.findings > 0) return "var(--chart-3)"
  return "var(--chart-2)"
}

export function languageColor(language: string): string {
  const normalized = language.toLowerCase()
  if (normalized.includes("typescript") || normalized === "tsx") {
    return "var(--chart-3)"
  }
  if (normalized.includes("javascript") || normalized === "jsx") {
    return "var(--chart-4)"
  }
  if (normalized.includes("python")) return "var(--chart-1)"
  if (normalized === "php") return "var(--chart-5)"
  if (normalized === "rust") return "oklch(0.68 0.17 50)"
  if (normalized === "go") return "oklch(0.72 0.14 215)"
  return "var(--chart-2)"
}
