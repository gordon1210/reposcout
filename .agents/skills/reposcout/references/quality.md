# Quality assessment

Use this reference when interpreting risk, complexity, maintainability, duplication, markers,
test presence, churn, or cleanup value.

## Establish evidence availability first

Check diagnostics, `analysis_profile.analyzers`, `summary.assessment.unavailable_signals`, and
completeness fields before interpreting a metric. A disabled analyzer is unavailable, not zero.
Rankings prioritize investigation; they do not prove a refactor is needed or that work should be
delegated.

## Risk

Risk algorithm `5` is a continuous ranking over raw SLOC, cyclomatic complexity, and churn:

```text
0.40 * sloc / (sloc + 1,000)
+ 0.40 * cyclomatic / (cyclomatic + 100)
+ 0.20 * commits / (commits + 20)
```

Confirm each entry's `algorithm_version` and raw inputs before comparing reports. Filename-based
test matching is informational and does not change the score. Treat reasons such as `large`,
`complex`, or `high churn` as explanations of the factors, not independent measurements.

## Complexity and maintainability

Function thresholds apply to individual callable scopes. File-level cyclomatic values sum
independent functions plus top-level decisions; `--fail-on max-cyclomatic>N` gates the single
worst function. Generic code formats can use heuristic complexity marked `approximate`; non-code
formats receive no complexity.

Maintainability Index uses a normalized `0..100` formula: `0..9` low, `10..19` moderate, and
`20..100` good. Halstead and grammar-derived values are RepoScout-internal signals, not guaranteed
cross-tool or cross-language equivalents.

## Duplication

The default duplication corpus excludes minified and recognized bundled/chunk output while
retaining those files in inventory and navigation. Check
`analysis_profile.duplication.artifact_policy`; `include` means the caller explicitly supplied
`--dup-include-artifacts` or equivalent configuration.

Inspect `summary.assessment.production_duplication` and
`summary.top_production_duplicates` first for product-code cleanup. Production evidence excludes
conventional test files, Rust split test modules named `tests.rs`, and direct Rust inline-test
regions. A compact production instance must retain at least `min_dup_lines` contiguous non-test
lines. Use `summary.top_duplicates`, full `duplicates`, or `--dup-details` only when the complete
configured health corpus or precise raw families matter.

Compact lists suppress substantially overlapping groups; they do not delete detector evidence.
Coverage uses physical-line/token unions, so exact/near overlap is not double-counted. A mixed
production/test family remains relevant when at least one instance touches production source.

When production evidence has `complete: false`, describe the percentage as observed partial
evidence. Call it a lower bound only when Type-2 skipped work is the sole gap; omitted source files
also change the denominator and can move the percentage either way.

## Tests, markers, and churn

Test presence is emitted only when supported repository configuration establishes a runner. Its
test-file count applies that runner's conventional filename and directory defaults within the scan
scope; neither the configuration nor the selected filenames are measured coverage. Report it as
test-discovery evidence and verify important paths with the project's real tests.

Markers are comment-aware for first-class languages and can use raw-text fallback elsewhere or on
parse failure. Churn counts Git history under configured bounds; it identifies frequently changed
areas but does not establish defect probability. `summary.top_hotspots` combines churn and
complexity as an investigation ranking, not a defect prediction. Confirm paths and source context
before acting on any of these signals.
