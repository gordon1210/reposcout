import type { FileReport } from "@/lib/types"

export type ExplorerFileCategory =
  "source" | "test" | "config" | "schema" | "entrypoint" | "generated"

export function classifyFile(file: FileReport): ExplorerFileCategory {
  const normalized = file.path.toLowerCase()
  const name = normalized.split("/").at(-1) ?? normalized
  if (file.skip_hint) return "generated"
  if (isTestFile(file, normalized, name)) return "test"
  if (isConfigFile(normalized, name)) return "config"
  if (isSchemaFile(normalized, name)) return "schema"
  if (isEntrypoint(name)) return "entrypoint"
  return "source"
}

function isTestFile(
  file: FileReport,
  normalized: string,
  name: string
): boolean {
  return (
    file.has_inline_tests ||
    /(^|[/_.-])(test|tests|spec|specs)([/_.-]|$)/.test(normalized) ||
    /(_test\.go|_test\.rs|\.test\.[^.]+|\.spec\.[^.]+|test\.php)$/.test(name)
  )
}

function isConfigFile(normalized: string, name: string): boolean {
  return (
    /(^|[/_.-])(config|configs|configuration)([/_.-]|$)/.test(normalized) ||
    /\.(json|jsonc|ya?ml|toml)$/.test(name) ||
    /^(cargo\.toml|composer\.json|package\.json|pyproject\.toml|tsconfig.*\.json)$/.test(
      name
    )
  )
}

function isSchemaFile(normalized: string, name: string): boolean {
  return (
    /\.(proto|graphql|gql|sql|xsd)$/.test(name) ||
    /(^|[/_.-])(schema|schemas)([/_.-]|$)/.test(normalized)
  )
}

function isEntrypoint(name: string): boolean {
  return (
    /^(main|index|app|artisan|lib)\.[^.]+$/.test(name) || name === "artisan"
  )
}
