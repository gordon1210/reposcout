# Agent metric semantics

This is a normative extension of the root [`AGENTS.md`](../../AGENTS.md). Read it completely before
changing line, marker, complexity, duplication, test-presence, risk, assessment, or diagnostic
behavior. The root instructions remain in force.

## Lines and markers

- Line metrics are syntax-aware for Rust, Python, JavaScript, TypeScript/TSX, Go, and PHP, using
  tree-sitter comment ranges for comment-only lines. Other formats use a quote-aware fallback and
  expose `line_metrics_approximate: true`; the summary counts them in
  `line_metrics_approximate_files`.
- Markers are comment-aware for first-class languages. When a syntax tree exists, only comment
  nodes contribute TODO/FIXME/HACK occurrences; identifiers, strings, template strings, Python
  docstrings, and TSX text do not. Other source formats, explicitly included content formats, and
  parse failures use the raw-text fallback.
- Health-excluded content carries no per-file marker facts or canonical marker findings. Marker
  health eligibility is part of the per-file cache profile.

## Complexity

Complexity is calculated per function and only for code.

- `summary.complexity.cyclomatic_*` and `cognitive_*` averages, maxima, and totals are computed over
  individual functions rather than whole files.
- `max_complexity` / `--max-complexity` defaults to 20 and acts like an ESLint reporting rule.
  `summary.complexity.functions_over_threshold` counts every callable above it;
  `summary.complexity_violations` keeps the worst top-N with `path`, `name`, and `line`.
  `summary.top_functions` is threshold-independent, and every callable remains in per-file
  `complexity.functions[]`.
- First-class callable scopes include named functions and methods, JavaScript arrows/function
  expressions, Rust closures, Python lambdas, Go function literals, and PHP closures/arrows.
  Anonymous scopes inherit binding names where possible and must not inflate the enclosing
  function.
- `--fail-on max-cyclomatic>N` gates on the single worst function.
- Complexity runs only for `LangInfo::is_code()` health-eligible files. Prose, data, markup, and
  style formats have `complexity: null`, `approximate: false`, and never enter
  churn-by-complexity hotspots.
- Generic code languages without a bundled grammar use heuristic file-level complexity marked
  approximate. They contribute to `mi_avg` and `mi_min`, but not function-level
  cyclomatic/cognitive statistics.
- Python comprehension clauses and JS/TS default values, logical assignments, optional chains,
  and nullish coalescing add control-flow paths. File-level cyclomatic values sum independent
  function scopes and top-level decisions. Cognitive complexity includes direct self-recursion.
- Maintainability Index uses Microsoft's normalized 0–100 formula with SLOC as the cross-language
  source-operation proxy: 0–9 low, 10–19 moderate, 20–100 good.
- Halstead arithmetic follows the published equations, but grammar-specific leaf-token
  classification makes it a RepoScout-internal signal, not a cross-tool or cross-language
  equivalent.

## Duplication

Duplication is structured, format-scoped, similarity-scored, and line-filtered.

### Corpus and compatibility

- The zero-config corpus is source/build files. `health_includes` adds selected content formats;
  `health_scope = "all"` restores every recognized format; `health_excludes` applies last.
- Minified and recognized bundled/chunk output is excluded from tokenization, coverage, and
  findings by default while remaining in inventory and navigation. `--dup-include-artifacts` or
  `duplication_include_artifacts = true` opts back in.
- `analysis_profile.duplication.artifact_policy` records that choice, and
  `analysis_profile.health` records the effective health policy. Both must match for baselines;
  reports without profile metadata are not baseline-compatible.
- Inventory metrics are never filtered by duplication or health policy.
- Exact matching preserves structured token kinds and values. Type-2 candidate shapes normalize
  identifier and literal categories, then verify every retained pair with a two-way identifier
  map.
- Exact formats are isolated by default. Compatible scope may combine JavaScript, TypeScript, and
  TSX. `mild` trivia filtering ignores whitespace but keeps comments; `weak` also ignores comments.

### Coverage and groups

- `summary.duplication.duplicated_pct` uses `analyzed_lines`, not repository-wide LOC, so excluded
  content cannot dilute the result. `duplicates.file_coverage` and `by_language` contain only
  eligible files and formats.
- Every group in `duplicates.exact` or `duplicates.near` carries `format`, precise instance ranges,
  and `similarity`. Exact groups score `1.0`; pair-oriented near groups are `< 1.0` and
  `>= near_dup_min_similarity`.
- `summary.top_duplicates` is the compact all-health-corpus rollup ranked by removable
  `duplicated_lines = lines * (copies - 1)`, with copies, similarity, and up to ten
  `path:start-end` locations. The first block is retained; a later block must add at least
  `min_dup_lines` contiguous uncovered lines in at least two instances.
- `summary.top_production_duplicates` applies the same compact policy after excluding conventional
  tests, including Rust split modules named `tests.rs`, and direct Rust inline-test-only families.
  An instance intersecting inline-test regions must retain at least `min_dup_lines` contiguous
  non-test lines. Mixed production/test families remain visible. Human table and Markdown reports
  use this projection by default.
- Compact projections must never delete or rewrite raw exact/near groups, coverage, canonical
  findings, or pair findings.
- Groups whose largest instance spans fewer than `min_dup_lines` lines, default 3, are removed even
  if one dense line exceeds `min_dup_tokens`.
- Physically overlapping instances in the same file are pruned before grouping so a sliding window
  across repetitive lines is not reported as multiple copies.
- The detector matches duplicated text, not extractability. Repeated import preambles and other
  legitimate repetition may still rank; locations let callers judge them.
- `dup::DuplicateCoverage` is the single source of physical-line and duplication-token union
  semantics. Line/token percentages, language statistics, file coverage, and `--by-dir` must not
  double-count exact/near overlap. Its token denominator is the structured duplication lexer, not
  tiktoken's `summary.tokens`.
- Full JSON carries stable pair-oriented `duplicates.findings`;
  `summary.top_duplicate_findings` is compact, and `--dup-details` expands human output.

### Type-2 bounds and completeness

- Candidate discovery uses rolling rename-invariant fingerprints, deterministic rare-first bucket
  scheduling, merged covered-diagonal lookup, and compact token-range overlap suppression before
  constructing report objects. Preserve these early-reduction properties.
- Each format pool is bounded to 10,000,000 seed pairs, 250,000 buffered compact matches, and
  10,000,000 suppression overlap checks.
- On a bound, retain useful verified partial groups but propagate the reason and omitted work
  through `Type2Diagnostics`, top-level `ScanDiagnostics`, human reports, capabilities, and
  `type2_progress`. Never present incomplete near-duplicate metrics as complete.
- Type-2 similarity weights exact tokens at 1.0, consistently renamed identifiers at 0.80, and
  changed literals of the same category at 0.70.

## Scouting signals and diagnostics

All scouting signals live in `summary` and are designed for agent decisions:

- `symbols` aggregates function/type/export counts from first-class files.
- `skip_candidates` lists generated, minified, bundled, and vendored files that are not worth
  reading, with the same `reason` exposed as each file's `skip_hint`.
- `test_presence` is omitted unless discovered manifests or runner configuration establish a
  supported test setup. Reported frameworks retain their evidence paths; configured runners use
  their conventional discovery defaults and report selected test-file counts within the evidence
  directory. Detection consumes the discovery universe, not only analyzed formats. For subpath
  scans, it also probes only the fixed supported runner filenames in target ancestors through the
  Git root; this supplies project context without widening the analyzed file scope. Candidate
  manifests use bounded no-follow reads. Aggregate output does not publish inferred source-to-test
  matches, `untested_*` fields, or matching-test risk reasons.
- `top_risks` uses algorithm 5: `0.40·size + 0.40·complexity + 0.20·churn`. Each continuous factor
  is `value / (value + half_saturation_anchor)` with anchors of 1,000 SLOC, cyclomatic 100, and 20
  commits. Entries carry `algorithm_version` and raw inputs; ties break by those inputs then path.
  Filename-based test matching does not change risk or cleanup scores.
- `summary.top_hotspots` ranks health-eligible code by churn and complexity;
  `summary.top_token_files` remains a complete-inventory ranking, while
  `top_source_token_files` drives the concise source-first human report.
- `assessment` is computed last in `aggregate()` from existing signals. It exposes
  `fits_context_known`, `fits_context`, `token_budget`, `cleanup_worth_complete`, `cleanup_worth`,
  `unavailable_signals`, and `reasons`. `DEFAULT_CONTEXT_BUDGET = 200_000`. Context fit uses
  `summary.source.tokens`; complete inventory remains in `summary.tokens` and `languages`.
- Assessment's `production_duplication` uses non-test code, excluding direct Rust `#[cfg(test)]`
  regions, and triggers above 15%. Its evidence records the `production-source` corpus,
  duplicated/analyzed lines,
  percentage, and completeness. Type-2 truncation or source discovery/read limits make it partial.
  It is a lower bound only when omitted Type-2 work is the sole gap; missing source can alter both
  numerator and denominator. Churn-only truncation does not affect duplication completeness.
- `source` totals and `top_source_token_files` drive concise human reports; language tables collapse
  non-source formats into one content rollup. Repository-wide totals and languages remain complete
  inventory.
- Top-level `diagnostics` records discovered, analyzed, unsupported, and unreadable files; bounded
  unsupported-path samples; walker errors; and Type-2 partial-work counts and reasons. An apparent
  absence must remain distinguishable from a scan gap or a lower-bound result.
