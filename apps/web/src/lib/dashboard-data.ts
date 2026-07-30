import type {
  FileReport,
  FindingRecord,
  LanguageStat,
  ScanReport,
} from "@/lib/types"

export function rankedFiles(report: ScanReport): FileReport[] {
  return [...report.files].sort(
    (left, right) =>
      right.tokens - left.tokens || left.path.localeCompare(right.path)
  )
}

export function averageFileCyclomatic(file: FileReport): number | undefined {
  const functions = file.complexity?.functions
  if (!functions?.length) return undefined

  return (
    functions.reduce((total, fn) => total + fn.cyclomatic, 0) / functions.length
  )
}

const severityRank: Record<string, number> = {
  error: 3,
  warning: 2,
  note: 1,
}

export function rankedFindings(report: ScanReport): FindingRecord[] {
  return [...report.finding_catalog.findings].sort(
    (left, right) =>
      (severityRank[right.severity] ?? 0) -
        (severityRank[left.severity] ?? 0) ||
      left.primary_location.path.localeCompare(right.primary_location.path) ||
      left.primary_location.start_line - right.primary_location.start_line
  )
}

export function markerTotal(markers: Record<string, number>): number {
  return Object.values(markers).reduce((total, count) => total + count, 0)
}

export function sourceLanguageRows(languages: LanguageStat[]): LanguageStat[] {
  const source = languages.filter((language) => language.source !== false)
  const content = languages.filter((language) => language.source === false)
  if (content.length === 0) return source

  return [
    ...source,
    content.reduce<LanguageStat>(
      (total, language) => ({
        ...total,
        files: total.files + language.files,
        bytes: total.bytes + language.bytes,
        loc: total.loc + language.loc,
        sloc: total.sloc + language.sloc,
        comment_lines: total.comment_lines + language.comment_lines,
        tokens: total.tokens + language.tokens,
      }),
      {
        name: `Other content (${content.length} formats)`,
        source: false,
        files: 0,
        bytes: 0,
        loc: 0,
        sloc: 0,
        comment_lines: 0,
        tokens: 0,
      }
    ),
  ]
}
