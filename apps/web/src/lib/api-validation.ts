import type { DaemonGraphResponse, DaemonSnapshot } from "@/lib/types"

type Check = (value: unknown) => boolean

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value)
}

const text: Check = (value) => typeof value === "string"
const number: Check = (value) =>
  typeof value === "number" && Number.isFinite(value)
const boolean: Check = (value) => typeof value === "boolean"
const revision: Check = (value) =>
  number(value) && Number.isSafeInteger(value) && Number(value) >= 0
const optional =
  (check: Check): Check =>
  (value) =>
    value === undefined || check(value)
const nullable =
  (check: Check): Check =>
  (value) =>
    value === null || check(value)
const array =
  (check: Check): Check =>
  (value) =>
    Array.isArray(value) && value.every(check)
const strings = array(text)
const numbers = (...keys: string[]): Record<string, Check> =>
  Object.fromEntries(keys.map((key) => [key, number]))
const object =
  (fields: Record<string, Check>): Check =>
  (value) =>
    isRecord(value) &&
    Object.entries(fields).every(([key, check]) => check(value[key]))
const dictionary =
  (check: Check): Check =>
  (value) =>
    isRecord(value) && Object.values(value).every(check)

// Check nested data before a response enters React state. Unknown properties
// remain allowed so additive schema updates do not break older dashboards.
const source = object(
  numbers("files", "bytes", "tokens", "loc", "sloc", "comment_lines")
)
const symbols = object(numbers("functions", "types", "exports"))
const fileRef = object({ path: text, tokens: number })
const duplicate = object({
  ...numbers("lines", "tokens", "similarity", "copies", "duplicated_lines"),
  locations: strings,
})
const functionComplexityFields = {
  name: text,
  ...numbers("line", "cyclomatic", "cognitive", "max_nesting"),
}
const functionHotspot = object({ path: text, ...functionComplexityFields })
const productionDuplication = object({
  corpus: text,
  ...numbers("duplicated_lines", "analyzed_lines", "duplicated_pct"),
  complete: boolean,
})
const summary = object({
  ...numbers(
    "files",
    "bytes",
    "tokens",
    "loc",
    "sloc",
    "comment_lines",
    "comment_ratio"
  ),
  source: optional(source),
  languages: array(
    object({
      name: text,
      source: optional(boolean),
      ...numbers("files", "bytes", "tokens", "loc", "sloc", "comment_lines"),
    })
  ),
  complexity: object(
    numbers(
      "cyclomatic_total",
      "cyclomatic_avg",
      "cyclomatic_max",
      "cognitive_total",
      "cognitive_avg",
      "cognitive_max",
      "mi_avg",
      "mi_min",
      "functions",
      "cyclomatic_threshold",
      "functions_over_threshold",
      "approximate_files"
    )
  ),
  duplication: object({
    ...numbers(
      "exact_groups",
      "near_groups",
      "duplicated_lines",
      "duplicated_pct",
      "duplicated_tokens",
      "analyzed_tokens",
      "duplicated_tokens_pct"
    ),
    analyzed_lines: optional(number),
  }),
  markers: dictionary(number),
  top_token_files: array(fileRef),
  top_source_token_files: optional(array(fileRef)),
  top_hotspots: array(
    object({ path: text, ...numbers("commits", "cyclomatic", "score") })
  ),
  top_functions: array(functionHotspot),
  complexity_violations: optional(array(functionHotspot)),
  top_duplicates: array(duplicate),
  top_production_duplicates: optional(array(duplicate)),
  symbols,
  test_presence: optional(
    object({
      frameworks: array(object({ name: text, evidence: text })),
      test_files: number,
    })
  ),
  top_risks: array(
    object({
      path: text,
      algorithm_version: optional(number),
      ...numbers("score", "sloc", "cyclomatic", "churn_commits"),
      reasons: strings,
    })
  ),
  assessment: object({
    fits_context: boolean,
    token_budget: number,
    cleanup_worth: text,
    production_duplication: optional(productionDuplication),
    reasons: strings,
  }),
})
const file = object({
  path: text,
  language: text,
  ...numbers(
    "bytes",
    "tokens",
    "loc",
    "sloc",
    "comment_lines",
    "comment_ratio"
  ),
  line_metrics_approximate: optional(boolean),
  complexity: optional(
    object({
      ...numbers(
        "cyclomatic",
        "cognitive",
        "max_nesting",
        "maintainability_index"
      ),
      functions: optional(
        array(
          object({
            ...functionComplexityFields,
            end_line: optional(number),
            symbol_key: optional(text),
          })
        )
      ),
    })
  ),
  imports: optional(strings),
  markers: optional(dictionary(number)),
  marker_occurrences: optional(
    array(
      object({
        marker: text,
        ...numbers("line", "column", "occurrence"),
        context_hash: text,
      })
    )
  ),
  churn: optional(
    object({
      ...numbers("commits", "authors"),
      first_commit: optional(text),
      last_commit: optional(text),
    })
  ),
  approximate: boolean,
  symbols: optional(symbols),
  skip_hint: optional(text),
  has_inline_tests: optional(boolean),
})
const location = object({
  path: text,
  ...numbers("start_line", "end_line"),
  start_column: optional(number),
  end_column: optional(number),
})
const findingCatalog = object({
  version: number,
  findings: array(
    object({
      fingerprint: text,
      identity: optional(text),
      kind: text,
      severity: text,
      message: text,
      primary_location: location,
      related_locations: optional(array(location)),
      metrics: optional(dictionary(number)),
    })
  ),
})
const diagnostics = object({
  ...numbers(
    "discovered_files",
    "analyzed_files",
    "unsupported_files",
    "unreadable_files",
    "walker_errors"
  ),
  ignore_files_rejected: optional(number),
  type1_analysis_partial: optional(boolean),
  type1_seed_pairs_skipped: optional(number),
  type1_pair_limit_reached: optional(boolean),
  type1_match_limit_reached: optional(boolean),
})
const graphNode = object({ path: text, ...numbers("fan_in", "fan_out") })
const graph = object({
  languages: strings,
  ...numbers("nodes", "edges", "unresolved_imports"),
  files: array(
    object({
      path: text,
      language: text,
      ...numbers("fan_in", "fan_out"),
      dependencies: optional(strings),
      dependents: optional(strings),
      focus_distance: optional(number),
      symbol_reach: optional(
        object({
          symbol_id: text,
          name: text,
          kind: text,
          relation: text,
          ...numbers("fan_in", "fan_out"),
        })
      ),
    })
  ),
  edge_list: array(object({ source: text, target: text, resolver: text })),
  symbols: optional(
    array(
      object({
        id: text,
        name: text,
        qualified_name: text,
        kind: text,
        path: text,
        language: text,
        ...numbers("line", "fan_in", "fan_out"),
      })
    )
  ),
  symbol_edges: optional(
    array(
      object({ source: text, target: text, relation: text, resolver: text })
    )
  ),
  unresolved_symbol_relations: optional(number),
  focus: optional(strings),
  unmatched_focus: optional(strings),
  direction: optional(text),
  depth: optional(number),
  cycles: array(strings),
  orphans: strings,
  top_depended: array(graphNode),
  most_dependent: array(graphNode),
  parse_errors: optional(number),
  config_errors: optional(number),
  config_files: optional(strings),
})
const context = object({
  ...numbers(
    "strategy_version",
    "planning_ms",
    "budget_tokens",
    "selected_tokens",
    "candidate_files",
    "omitted_files",
    "skipped_files"
  ),
  focus: optional(strings),
  change_scope: optional(text),
  changed_files: optional(strings),
  graph_languages: optional(strings),
  graph_unresolved_imports: optional(number),
  graph_parse_errors: optional(number),
  graph_config_errors: optional(number),
  outline_symbols: optional(number),
  outline_bytes: optional(number),
  outline_omitted_symbols: optional(number),
  planning_diagnostics: optional(diagnostics),
  files: array(
    object({
      path: text,
      ...numbers("tokens", "score"),
      reasons: strings,
      evidence: optional(
        array(
          object({
            role: text,
            confidence: text,
            distance: optional(number),
            resolver: optional(text),
          })
        )
      ),
      symbols: optional(
        array(
          object({
            name: text,
            kind: text,
            signature: text,
            line: number,
            exported: optional(boolean),
            reasons: optional(strings),
          })
        )
      ),
    })
  ),
  omitted: optional(
    array(object({ path: text, tokens: number, reason: text }))
  ),
})
const report = object({
  schema_version: (value) => value === "2.0",
  root: text,
  target: text,
  generated_at: text,
  analysis_profile: optional(
    nullable(
      object({
        analyzers: object({
          tokens: boolean,
          complexity: boolean,
          imports: boolean,
          markers: boolean,
          duplication: boolean,
          churn: boolean,
        }),
        health: optional(
          object({
            scope: (value) => value === "source" || value === "all",
            includes: optional(strings),
          })
        ),
      })
    )
  ),
  summary,
  files: array(file),
  finding_catalog: findingCatalog,
  diagnostics,
  graph: optional(nullable(graph)),
  context: optional(nullable(context)),
})
const snapshot = object({
  target: text,
  profile: text,
  revision,
  status: (value) =>
    typeof value === "string" &&
    ["starting", "scanning", "ready", "error"].includes(value),
  scan_started_at: nullable(text),
  scan_finished_at: nullable(text),
  error: nullable(text),
  report: nullable(report),
})
const graphResponse = object({ revision, graph })

export function isDaemonSnapshot(value: unknown): value is DaemonSnapshot {
  return snapshot(value)
}

export function isDaemonGraphResponse(
  value: unknown
): value is DaemonGraphResponse {
  return graphResponse(value)
}

function withEmptyGraphCollections(value: unknown): unknown {
  if (!isRecord(value)) return value
  return {
    ...value,
    files: value.files === undefined && value.nodes === 0 ? [] : value.files,
    edge_list:
      value.edge_list === undefined && value.edges === 0 ? [] : value.edge_list,
  }
}

// Rust omits empty graph collections on the wire; the UI consumes explicit arrays.
export function parseDaemonGraphResponse(
  value: unknown
): DaemonGraphResponse | null {
  if (!isRecord(value)) return null
  const candidate = { ...value, graph: withEmptyGraphCollections(value.graph) }
  return isDaemonGraphResponse(candidate) ? candidate : null
}

export function parseDaemonSnapshot(value: unknown): DaemonSnapshot | null {
  if (!isRecord(value)) return null
  const candidate =
    isRecord(value.report) && value.report.graph != null
      ? {
          ...value,
          report: {
            ...value.report,
            graph: withEmptyGraphCollections(value.report.graph),
          },
        }
      : value
  return isDaemonSnapshot(candidate) ? candidate : null
}
