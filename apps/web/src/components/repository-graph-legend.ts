import type {
  ExplorerLanguageStat,
  ExplorerView,
} from "@/lib/graph-explorer-model"
import type { GraphProminence } from "@/lib/graph-data"

export interface CanvasLegendData {
  languages: ExplorerLanguageStat[]
  moreLanguages: number
  hasTypeRelations: boolean
  hasImportContext: boolean
  hasExternal: boolean
  hasProminent: boolean
}

export function buildCanvasLegend(
  view: ExplorerView,
  prominence: Map<string, GraphProminence>
): CanvasLegendData {
  const languages = collectLegendLanguages(view)
  const typeRelations = new Set(["extends", "implements", "embeds"])
  return {
    languages: languages.slice(0, 5),
    moreLanguages: Math.max(0, languages.length - 5),
    hasTypeRelations: view.connections.some((connection) =>
      typeRelations.has(connection.relation)
    ),
    hasImportContext:
      view.presentation === "type" &&
      view.connections.some(
        (connection) => !typeRelations.has(connection.relation)
      ),
    hasExternal: view.entities.some((entity) => entity.external),
    hasProminent: [...prominence.values()].some(
      (entry) => entry.level !== "standard"
    ),
  }
}

function collectLegendLanguages(view: ExplorerView): ExplorerLanguageStat[] {
  const languages = new Map<string, number>()
  for (const entity of view.entities) {
    if (entity.kind === "file") {
      languages.set(
        entity.report.language,
        (languages.get(entity.report.language) ?? 0) + 1
      )
      continue
    }
    for (const stat of entity.languages) {
      languages.set(stat.name, (languages.get(stat.name) ?? 0) + stat.files)
    }
  }
  return [...languages.entries()]
    .map(([name, files]) => ({ name, files }))
    .sort(
      (left, right) =>
        right.files - left.files || left.name.localeCompare(right.name)
    )
}
