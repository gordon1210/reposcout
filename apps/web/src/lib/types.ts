export type DaemonStatus = "starting" | "scanning" | "ready" | "error"

export interface DaemonSnapshot {
  target: string
  profile: "lite" | "full" | string
  revision: number
  status: DaemonStatus
  scan_started_at: string | null
  scan_finished_at: string | null
  error: string | null
  report: ScanReport | null
}

export interface ScanReport {
  schema_version: string
  root: string
  target: string
  generated_at: string
  analysis_profile?: AnalysisProfile | null
  summary: Summary
  files: FileReport[]
  finding_catalog: FindingCatalog
  diagnostics: ScanDiagnostics
  graph?: DependencyGraph | null
  context?: ContextPlan | null
}

export interface DaemonGraphResponse {
  revision: number
  graph: DependencyGraph
}

export interface DependencyGraph {
  languages: string[]
  nodes: number
  edges: number
  files: GraphFile[]
  edge_list: GraphEdge[]
  symbols?: GraphSymbol[]
  symbol_edges?: GraphSymbolEdge[]
  unresolved_symbol_relations?: number
  focus?: string[]
  unmatched_focus?: string[]
  direction?: string
  depth?: number
  cycles: string[][]
  orphans: string[]
  top_depended: GraphNode[]
  most_dependent: GraphNode[]
  unresolved_imports: number
  parse_errors?: number
  config_errors?: number
  config_files?: string[]
}

export interface GraphFile {
  path: string
  language: string
  fan_in: number
  fan_out: number
  dependencies?: string[]
  dependents?: string[]
  focus_distance?: number
  symbol_reach?: GraphSymbolReach
}

export interface GraphEdge {
  source: string
  target: string
  resolver: string
}

export interface GraphSymbol {
  id: string
  name: string
  qualified_name: string
  kind: string
  path: string
  language: string
  line: number
  fan_in: number
  fan_out: number
}

export interface GraphSymbolEdge {
  source: string
  target: string
  relation: "extends" | "implements" | "embeds" | string
  resolver: "qualified" | "same-file" | "same-scope" | "unique-name" | string
}

export interface GraphSymbolReach {
  symbol_id: string
  name: string
  kind: string
  fan_in: number
  fan_out: number
  relation: string
}

export interface GraphNode {
  path: string
  fan_in: number
  fan_out: number
}

export interface ContextPlan {
  strategy_version: number
  planning_ms: number
  budget_tokens: number
  selected_tokens: number
  candidate_files: number
  omitted_files: number
  skipped_files: number
  focus?: string[]
  change_scope?: string
  changed_files?: string[]
  graph_languages?: string[]
  graph_unresolved_imports?: number
  graph_parse_errors?: number
  graph_config_errors?: number
  outline_symbols?: number
  outline_bytes?: number
  outline_omitted_symbols?: number
  planning_diagnostics?: ScanDiagnostics
  files: ContextFile[]
  omitted?: ContextOmission[]
}

export interface ContextFile {
  path: string
  tokens: number
  score: number
  reasons: string[]
  evidence?: ContextEvidence[]
  symbols?: SymbolOutline[]
}

export interface ContextEvidence {
  role: string
  confidence: string
  distance?: number
  resolver?: string
}

export interface SymbolOutline {
  name: string
  kind: string
  signature: string
  line: number
  exported?: boolean
  reasons?: string[]
}

export interface ContextOmission {
  path: string
  tokens: number
  reason: string
}

export interface AnalysisProfile {
  analyzers: {
    tokens: boolean
    complexity: boolean
    imports: boolean
    markers: boolean
    duplication: boolean
    churn: boolean
  }
  health?: {
    scope: "source" | "all"
    includes?: string[]
  }
}

export interface SourceSummary {
  files: number
  bytes: number
  tokens: number
  loc: number
  sloc: number
  comment_lines: number
}

export interface Summary {
  files: number
  bytes: number
  tokens: number
  loc: number
  sloc: number
  comment_lines: number
  comment_ratio: number
  source?: SourceSummary
  languages: LanguageStat[]
  complexity: ComplexitySummary
  duplication: DuplicationSummary
  markers: Record<string, number>
  top_token_files: FileRef[]
  top_source_token_files?: FileRef[]
  top_hotspots: Hotspot[]
  top_functions: FunctionHotspot[]
  complexity_violations: FunctionHotspot[]
  top_duplicates: DuplicateBlock[]
  symbols: SymbolCounts
  test_presence: TestPresence
  top_risks: RiskEntry[]
  assessment: Assessment
}

export interface LanguageStat {
  name: string
  source?: boolean
  files: number
  bytes: number
  loc: number
  sloc: number
  comment_lines: number
  tokens: number
}

export interface ComplexitySummary {
  cyclomatic_total: number
  cyclomatic_avg: number
  cyclomatic_max: number
  cognitive_total: number
  cognitive_avg: number
  cognitive_max: number
  mi_avg: number
  mi_min: number
  functions: number
  cyclomatic_threshold: number
  functions_over_threshold: number
  approximate_files: number
}

export interface DuplicationSummary {
  exact_groups: number
  near_groups: number
  duplicated_lines: number
  duplicated_pct: number
  analyzed_lines?: number
  duplicated_tokens: number
  analyzed_tokens: number
  duplicated_tokens_pct: number
}

export interface FunctionHotspot {
  path: string
  name: string
  line: number
  cyclomatic: number
  cognitive: number
  max_nesting: number
}

export interface DuplicateBlock {
  lines: number
  tokens: number
  similarity: number
  copies: number
  duplicated_lines: number
  locations: string[]
}

export interface SymbolCounts {
  functions: number
  types: number
  exports: number
}

export interface TestPresence {
  test_files: number
  source_files: number
  untested_source_files: number
  untested_samples: string[]
}

export interface RiskEntry {
  path: string
  score: number
  sloc: number
  cyclomatic: number
  churn_commits: number
  untested: boolean
  reasons: string[]
}

export interface Assessment {
  fits_context: boolean
  token_budget: number
  cleanup_worth: "low" | "medium" | "high" | string
  reasons: string[]
}

export interface FileRef {
  path: string
  tokens: number
}

export interface Hotspot {
  path: string
  commits: number
  cyclomatic: number
  score: number
}

export interface FileReport {
  path: string
  language: string
  bytes: number
  tokens: number
  loc: number
  sloc: number
  comment_lines: number
  comment_ratio: number
  line_metrics_approximate?: boolean
  complexity?: {
    cyclomatic: number
    cognitive: number
    max_nesting: number
    maintainability_index: number
    functions?: FunctionComplexity[]
  }
  imports?: string[]
  markers?: Record<string, number>
  marker_occurrences?: MarkerOccurrence[]
  churn?: {
    commits: number
    authors: number
    first_commit?: string
    last_commit?: string
  }
  approximate: boolean
  symbols?: SymbolCounts
  skip_hint?: string
  has_inline_tests?: boolean
}

export interface FunctionComplexity {
  name: string
  line: number
  end_line?: number
  symbol_key?: string
  cyclomatic: number
  cognitive: number
  max_nesting: number
}

export interface MarkerOccurrence {
  marker: string
  line: number
  column: number
  context_hash: string
  occurrence: number
}

export interface FindingCatalog {
  version: number
  findings: FindingRecord[]
}

export interface FindingRecord {
  fingerprint: string
  identity?: string
  kind: string
  severity: string
  message: string
  primary_location: {
    path: string
    start_line: number
    end_line: number
    start_column?: number
    end_column?: number
  }
  related_locations?: Array<{
    path: string
    start_line: number
    end_line: number
    start_column?: number
    end_column?: number
  }>
  metrics?: Record<string, number>
}

export interface ScanDiagnostics {
  discovered_files: number
  analyzed_files: number
  unsupported_files: number
  unreadable_files: number
  walker_errors: number
}
