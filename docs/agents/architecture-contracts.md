# Agent architecture contracts

This is a normative extension of the root [`AGENTS.md`](../../AGENTS.md). Read it completely before
changing scanner orchestration, serialized models, cacheable file facts, discovery policy, or the
duplication interfaces. The root instructions remain in force.

## Stable model and analyzer interfaces

`src/model.rs` is the stable serializable contract. Every analyzer writes into these structs and
every reporter reads from them. Treat field shapes as an API: bump `SCHEMA_VERSION` only for a
breaking JSON change and update integration tests with it.

Analyzers are decoupled behind these signatures. Keep them stable; if a change is unavoidable,
update `scan.rs`, the only caller, and tests together:

```rust
// metrics/complexity.rs
pub fn analyze(lang: &LangInfo, content: &str, tree: Option<&Tree>, lines: &LineStats)
    -> (Complexity, bool);            // bool = approximate heuristic fallback
pub fn maintainability_index(halstead: &Halstead, cyclomatic: u32, lines: &LineStats) -> f64;

// metrics/imports.rs
pub fn extract(fc: FirstClass, content: &str, tree: &Tree) -> Vec<String>;

// dup/exact.rs
pub fn detect(inputs: &[DupInput], min_tokens: usize) -> Vec<CloneGroup>;
// dup/fuzzy.rs
pub fn detect(inputs: &[DupInput], min_tokens: usize, min_similarity: f64) -> Vec<CloneGroup>;

// git.rs
pub fn collect(root: &Path, files: &[PathBuf], max_commits: usize) -> HashMap<PathBuf, Churn>;
```

The two `dup::{exact,fuzzy}::detect` signatures are the **frozen detector contract**. Wrappers in
`dup/mod.rs` are not frozen. `dup::analyze_with_progress` is the public coarse-stage wrapper;
`dup::analyze_with_diagnostics` also carries bounded Type-2 progress/completeness into the scanner.
The orchestration prepares one structured token corpus, runs both prepared detectors, applies
detector-specific cleanup, creates pair findings, and retains the token denominator for union
coverage. Cross-cutting duplication behavior belongs there, not in the detector adapters.

## Compatibility and cache invalidation

- Additive `model.rs` fields use `#[serde(default)]`, plus `skip_serializing_if` for optional,
  vector, and boolean fields where appropriate, so older reports, caches, and baselines still
  deserialize without a schema bump.
- Bump `ANALYZER_VERSION` in `cache.rs` whenever the contents or semantics of a cached `FileReport`
  change, including new symbols, inline-test facts, skip hints, or report-path semantics.
- `AnalysisProfile` must contain every runtime setting that changes a cached file report: token
  encoding/enabled state, complexity, imports, effective marker set, health eligibility, and
  `health_excludes`. Summary-only and top-level projections such as rollups, baselines, findings,
  graph, context, diagnostics, review, and impact do not require an analyzer bump.
- Precise marker occurrences are per-file facts and therefore participate in cache invalidation.
- `imports::extract` returns root module names only (`std`, `crate`, `os`, `node:fs`), not
  resolvable local paths. `graph.rs` owns relative-import extraction and resolution.

## Scan data flow

The core flow is:

```text
main
  -> scan::run_with_exclusions(target, cfg, output_paths)
  -> walk discovery and stable identities
  -> parallel per-file analysis in the configured Rayon pool
  -> profile-valid cache
  -> duplication token preparation in that same pool
  -> dup::analyze_with_diagnostics and DuplicateCoverage
  -> git churn, aggregate Summary, canonical findings, diagnostics
  -> optional review / directory rollup / baseline / graph / context / impact
  -> ScanReport
  -> report::render
```

`report/agent_summary.rs` is a pure, hard-bounded projection over `ScanReport`. It may classify and
trim already-produced evidence, but must never trigger analyzers, rebuild topology, or become a
second task-query implementation.

`scan::run` remains the no-exclusions library wrapper. Diff-scoped context deliberately analyzes a
separate cached full-tree planning universe after the primary scoped scan. This supplies unchanged
tests, dependents, risks, and symbol outlines without widening `summary`, `files`, or findings; its
topology is reused by impact when both modes run.

`reposcout explain FILE` is intentionally separate: it scans the surrounding root, then projects
the requested file's discovery, risk, tests, graph adjacency, and findings into `ExplainReport`.
Do not introduce another analyzer pipeline for it.

## Language and health scope

First-class tree-sitter languages are Rust, Python, JavaScript, TypeScript/TSX, Go, and PHP. Every
recognized format contributes to complete inventory, token/context size, and line facts.

Health analysis defaults to programming/build source. HTML, CSS/SCSS, JSON, YAML, TOML, Markdown,
XML, and text require `health_includes` / `--health-include` or explicit
`health_scope = "all"` / `--health-scope all`.
`health_excludes` / `--health-exclude` then removes repository-relative globs from complexity,
markers, duplication, risk, test-presence, and cleanup signals while inventory, tokens, lines,
imports, symbols, and context discovery stay complete. Apply scope first, format includes second,
and path excludes last; path excludes win.

Generic code languages use heuristic complexity with `approximate: true`. Non-code formats do not
receive complexity. Never let health filtering silently change complete inventory semantics.

## Discovery, configuration, outputs, and cache placement

- Output files must not feed back into a scan. The CLI passes `-o/--output` as an exact canonical
  filesystem exclusion to both scoped discovery and impact topology. Do not replace it with a glob
  that could hide lookalike paths.
- Caching never writes `.reposcout/` or other state into the scanned repository. `cache.rs` uses the
  OS cache directory through `directories::ProjectDirs`, keyed by a hash of the canonical scan
  root.
- Declaration outlines are cached independently of context output. Graph source facts enrich the
  same entry lazily for CLI graph/context/impact/explain work and do not change `AnalysisProfile`.
  Daemon refreshes deliberately capture those facts plus bounded resolver-config contents so a
  revision-keyed graph never rereads mutable live sources; they still do not build graph topology.
- `reposcout cache clear [PATH]` removes analysis and churn data for the canonical scan root;
  `--all` explicitly removes the application cache directory. Keep reset idempotent and scoped by
  default.
- Configuration precedence is CLI flags, nearest project `reposcout.toml` or `.reposcout.toml`,
  OS-appropriate global `reposcout.toml`, then defaults. Optional file fields merge by presence;
  nested `[context]` fields merge independently. Arrays replace lower layers, while CLI
  `--exclude` and `--health-include` extend their effective lists.
- `REPOSCOUT_GLOBAL_CONFIG` is the hermetic global-path override for automation and tests.
  `reposcout config [PATH]` reports sources, ignored/loaded keys, precedence, and final
  file-configurable values. `--no-project-config` is the repository-config trust boundary.
- The `agent` profile disables duplication and churn by default. `safe` also enforces conservative
  worker, history, context, duplication, discovery, and project-config guardrails and forces source
  health scope with no content includes. Explicit analyzer selection may opt an analyzer back in;
  no profile promises a total runtime bound for an arbitrarily large target.
- `.reposcoutignore` uses gitignore syntax, is hierarchical per directory, and is added through
  `add_custom_ignore_filename` in `walk.rs`. It remains active under `--no-ignore` and is the right
  place to exclude generated or vendored trees from scouting.
- Lockfiles are excluded by default through `exclude_lockfiles` and `LOCKFILES` in `walk.rs`.
  `--include-lockfiles` or `exclude_lockfiles = false` re-includes recognized lockfiles. `.lock`
  and `.sum` remain unsupported formats; the policy mainly affects files such as
  `package-lock.json` and `pnpm-lock.yaml`.
