# AGENTS.md

Guidance for coding agents (and humans) working in the **reposcout** repository.

`reposcout` is a Rust CLI that scans a git repository — or any path inside it — and
prints a fast, consolidated status: token counts, complexity, duplication, line
metrics, markers, imports, and git churn. See `README.md` for user-facing docs.

---

## ⚠️ Keep the development binary up to date

A global `reposcoutdev` command is installed as a **symlink** on this machine:

```
~/.local/bin/reposcoutdev  ->  <repo>/target/release/reposcout
```

`~/.local/bin` is on `PATH`, so `reposcoutdev` runs from anywhere. The `reposcout`
command is reserved for an installed public release and must not point into this
working tree. Because `reposcoutdev` is a symlink to the **release** build, it
reflects the latest development binary as soon as you rebuild in release mode.

**After making ANY code change, rebuild the release binary so the development
command stays current:**

```sh
cargo build --release
```

If the symlink is ever missing or broken (e.g. after `cargo clean` or moving the
repo), recreate it:

```sh
mkdir -p ~/.local/bin
ln -sf "$(pwd)/target/release/reposcout" ~/.local/bin/reposcoutdev
reposcoutdev --version   # verify
```

> Do **not** rely on the debug build (`target/debug`) for the development command —
> the symlink points at `target/release`. A debug-only `cargo build` will not update it.

---

## Long-running process ownership

- Never start, stop, restart, signal, kill, or otherwise control `reposcout daemon`, a
  frontend development server, Vite, a file watcher, a browser, or any other long-running
  process unless the user explicitly authorizes that process interaction in the current task.
- Treat every pre-existing or unknown process as user-owned. A listening port, matching command
  name, repository path, or apparent relationship to the task is never evidence that an agent
  owns the process.
- When the user explicitly authorizes an agent to start a temporary process, record its exact
  session or PID. Cleanup may target only that recorded process; never use broad process-name or
  port-based termination, and never terminate a process whose ownership is uncertain.
- Do not start daemons or frontends merely for routine validation. Prefer one-shot tests and
  production builds. If live validation is necessary but not explicitly authorized, report that
  it was not performed instead of starting or touching a long-running process.

---

## Resource-safety guardrails

RepoScout integration tests launch the compiled CLI, so the suite enforces safe defaults in code
and repository configuration: `.cargo/config.toml` sets `RUST_TEST_THREADS=1`, the shared test
command uses `tests/fixtures/test-global.toml` to cap RepoScout at two worker threads, and tests use
bounded fixtures rather than scanning this repository. Bare `cargo test` is therefore the intended
full-suite command.

- Do not override `RUST_TEST_THREADS`, pass a larger `--test-threads` value, or bypass the shared
  `tests/support/command.rs` helper during routine validation. A direct CLI child needed for a
  process-I/O test must use the same `test_global_config()` path.
- Keep integration-test targets synthetic and bounded. Do not reintroduce scans of
  `CARGO_MANIFEST_DIR` merely to obtain a large or representative report.
- Run at most one build, test suite, benchmark, or RepoScout scan at a time. Do not parallelize
  resource-intensive validation in shell jobs, tool calls, or sub-agents.
- Do not run an unbounded RepoScout scan or benchmark against a large external repository without
  explicit user authorization. Prefer a focused target and bounded history for routine checks.
- Record the exact session/PID for every manually authorized resource-intensive command and monitor it. If
  it unexpectedly runs longer than 60 seconds, exceeds 1 GiB RSS, or spawns more than one
  RepoScout CLI child, stop only that recorded agent-owned session immediately and report what
  happened. Do not let it continue merely to complete a checklist.
- If resource-intensive validation is stopped by a guardrail, run the smallest relevant targeted
  tests and clearly report which broader checks were not run.

---

## Toolchain & environment

- Rust (edition **2024**), installed via `rustup`. If `cargo` is not found in a
  fresh shell, load it first:

  ```sh
  source "$HOME/.cargo/env"
  ```

- A C compiler is required (vendored `libgit2` and the tree-sitter grammars build
  via `cc`). `cmake` is **not** needed.
- Frontend packages use the root pnpm workspace (`pnpm@11.18.0`).

## Common commands

```sh
cargo build                 # debug build
cargo build --release       # release build (refreshes the reposcoutdev symlink)
cargo test                  # full suite; serialized automatically by .cargo/config.toml
cargo test <FILTER>         # targeted test; inherits the same safe default
cargo clippy --all-targets -- -D warnings
cargo fmt                   # format (run before committing)
cargo run -- -f json .      # run against this repo
pnpm lint:frontend          # shared ESLint config + both frontend apps
pnpm lint:fix:frontend      # apply ESLint and Prettier fixes across frontend packages
pnpm build:web              # type-check + production dashboard build
pnpm test:web               # dashboard Vitest suite
pnpm build:landing          # type-check + production landing-page build
```

## Repository layout

```
CHANGELOG.md          User-visible changes in reverse chronological order.
apps/
  web/                React dashboard for the live daemon.
  landing/            Bespoke public RepoScout landing page.
packages/
  eslint-config/      Shared flat ESLint configuration for both frontend apps.
src/
  main.rs            CLI entry: scan/query/explain dispatch, profiles, gates, output/errors,
                     and debug-session lifecycle.
  lib.rs             Re-exports the modules below (so tests/bin share one crate).
  cli.rs             clap definitions (Cli, Command, ScanArgs, OutputFormat).
  config.rs          Layered defaults/global/project config resolution + inspection.
  debug_log.rs       Opt-in, immediately flushed NDJSON run diagnostics, quiet-work heartbeat,
                     process-memory sampling, and panic capture.
  context.rs         Deterministic, token-budgeted, focus/change-aware reading-plan ranking
                     plus bounded structural-outline projection.
  work_scope.rs      Additive bounded projection of observed repository, seed, context, impact,
                     graph-component, and confidence facts for agent decisions.
  model.rs           THE shared data contract (all serde result structs).
  lang.rs            Language detection by extension/filename.
  walk.rs            .gitignore-aware discovery (ignore crate) + git root +
                     .reposcoutignore custom ignore file + stable report/cache paths
                     and walker-error accounting.
  scan.rs            Orchestrator: parallel per-file analysis, cache, aggregation,
                     canonical findings, directory rollup, baseline/review, diff scopes,
                     diagnostics, and change impact.
  findings.rs        Canonical complexity/marker/duplication/risk finding projection,
                     stable fingerprints, four-way comparison, and rename remapping.
  review.rs          Fast changed-line filtering and deep Git-snapshot comparison.
  snapshot.rs        Worktree/tree/index source adapters plus shared snapshot policy.
  explain.rs         Full-repository scan projected onto one requested file.
  query.rs           Deep task-query interface: capabilities and bounded symbol lookup.
  cache.rs           Incremental cache in the OS cache dir (keyed by scan root), plus
                     repository/all-scope manual reset; analysis profile + ANALYZER_VERSION
                     invalidate per-file facts.
  parse/mod.rs       tree-sitter Language + parse() for first-class languages.
  php.rs             Shared PHP namespace-use and static include syntax normalization.
  graph.rs           Opt-in (--graph/--impact/explain/context) structural graph for every
                     first-class language: reusable import adjacency/signals, explicit type
                     relations, stable resolver-provenance edges, language package/module
                     metadata, bounded focus queries, cycles, orphans, virtual deleted nodes,
                     and reverse impact.
  graph/symbols.rs   Conservative cross-file collector for syntax-proven extends, implements,
                     trait, and embedding relationships across first-class languages.
  metrics/
    tokens.rs        tiktoken token counting (o200k_base / cl100k_base).
    lines.rs         LOC / SLOC / comment / blank line stats.
    markers.rs       Comment-aware TODO/FIXME/HACK/... counting (AST + fallback).
    complexity.rs    Cyclomatic, cognitive, nesting, Halstead, MI (+ per-function).
    imports.rs       Import / dependency extraction (ROOT module names only).
    symbols.rs       Per-file symbol counts plus compact declaration headers from the AST.
    classify.rs      "Don't-read" skip-hint heuristics (generated/minified/vendored).
    testcov.rs       Test-vs-source classification + filename/PHPUnit/Rust CLI/inline-test
                     matching and direct `cfg(test)` region detection.
    risk.rs          Shared composite risk calculation and explain factors.
  dup/
    mod.rs           Prepared-corpus orchestration, format pools, rolling-hash helpers,
                     cleanup, detailed findings, and union line/token coverage.
    tokenize.rs      Structured AST/fallback lexer with precise source ranges.
    exact.rs         Format-scoped rolling-hash Type-1 clone detection.
    fuzzy.rs         Type-2 detection with rolling rename-invariant fingerprints,
                     pair-local identifier bijection, bounded candidate verification, merged
                     diagonal coverage, compact overlap suppression, and detailed progress.
    fuzzy/plan.rs    Deterministic rare-first admission control and work accounting for
                     repetitive Type-2 fingerprint buckets.
  git.rs             Churn plus changed-file/changed-line Git diffs and rename detection.
  report/            json.rs, table.rs, markdown.rs, sarif.rs (SARIF 2.1.0),
                     ndjson.rs, graph.rs (DOT/Mermaid), config.rs, and focused explain/query
                     renderers.
tests/
  cli.rs             End-to-end integration tests against fixtures.
  dup_languages.rs   Public detector + CLI matrix for every canonical format.
  support/           Shared fixture-manifest materialization helpers.
  fixtures/dup_languages.toml
                     Exact and Type-2 samples for all 31 detected formats.
  fixtures/sample/   Small multi-language fixture tree.
```

`apps/web/src/components/ui/` contains imported shadcn primitives. It is globally ignored by
ESLint and must not be edited by hand.

Frontend production code is linted with a cyclomatic-complexity ceiling of 20 and a 900-line
module ceiling. Tests keep correctness and formatting checks but are exempt from size,
complexity, strict assertion, and development-only React rules. The dashboard keeps its runtime
shell in `dashboard.tsx` and report rendering in `dashboard-report.tsx`; the repository graph is
split across controller, workspace view, canvas renderers, layout decoration, detail panels, and
pure graph helpers under `apps/web/src/components/repository-graph-*` and `apps/web/src/lib/graph-*`.

## Architecture & the frozen contract

`src/model.rs` is the **stable, serializable contract**. Every analyzer writes into
these structs; every reporter reads from them. Treat the field shapes as an API —
`SCHEMA_VERSION` must be bumped for breaking JSON changes, and the integration
tests assert on this shape.

Analyzers are decoupled behind these signatures. **Keep them stable**; if you must
change one, update `scan.rs` (the only caller) and the tests together:

```rust
// metrics/complexity.rs
pub fn analyze(lang: &LangInfo, content: &str, tree: Option<&Tree>, lines: &LineStats)
    -> (Complexity, bool);            // bool = `approximate` (heuristic fallback)
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

The two `dup::{exact,fuzzy}::detect` signatures above are the **frozen** detector
contract. The orchestration wrappers in `dup/mod.rs` are *not* frozen.
`dup::analyze_with_progress` is the public coarse-stage wrapper;
`dup::analyze_with_diagnostics` additionally carries bounded Type-2 progress/completeness into the
scanner. The orchestration prepares one structured token corpus, runs both prepared detectors,
applies detector-appropriate cleanup, builds pair findings, and retains the duplication-token
denominator needed for union coverage. Add cross-cutting duplication behavior there rather than
changing the public detector adapters.

Two more contract rules that keep JSON stable:

- **Additive fields only, guarded with serde defaults.** New `model.rs` fields use
  `#[serde(default)]` (plus `skip_serializing_if` for `Option`/`Vec`/`bool`) so old
  cached/baseline JSON still deserializes and `SCHEMA_VERSION` can stay `1.0`. Bump
  `SCHEMA_VERSION` only for a *breaking* change and update the integration tests with it.
- **Bump `ANALYZER_VERSION` (in `cache.rs`) when per-file analysis changes.** The cache key
  includes it, so changing what goes into a `FileReport` (e.g. adding `symbols`,
  `has_inline_tests`, changing `skip_hint`, or changing report-path semantics) requires a
  bump to invalidate stale entries. `AnalysisProfile` must also include every runtime setting
  that changes a cached `FileReport`: token encoding/enablement, complexity, imports, and the
  effective marker set plus health-file eligibility and `health_excludes`. Features that only add *summary* or top-level fields (rollup, baseline,
  finding catalog, graph, context, diagnostics, review, impact) do **not** need a bump. Precise marker
  occurrences are per-file facts, so their introduction required an analyzer-version bump.
- `imports::extract` returns only **root** module names (`std`, `crate`, `os`, `node:fs`),
  not resolvable local paths. The dependency graph therefore does its own relative-import
  extraction and resolution in `graph.rs` rather than reusing it.

Data flow: `main` → `scan::run_with_exclusions(target, cfg, output_paths)` discovers stable
file identities (`walk`; `scan::run` remains the no-exclusions library wrapper),
analyzes each in parallel (`rayon`), consults a profile-valid `cache`, runs cross-file
`dup::analyze_with_progress` and builds `DuplicateCoverage`, attaches `git::collect`
churn, aggregates a `Summary`, projects the complete canonical finding catalog, records scan
diagnostics, optionally builds a changed-line review / directory rollup / baseline delta /
dependency graph / token-budgeted context plan / diff impact, and returns a
`ScanReport` that `report::render` turns into table / JSON / markdown / SARIF / NDJSON or a
graph-only DOT / Mermaid projection.
Diff-scoped context deliberately analyzes a separate, cached full-tree planning universe after
the primary scoped scan; this supplies unchanged tests/dependents/risk and internal symbol outlines
without widening summary/files/findings. Its topology is also reused by impact when both modes run.
`reposcout explain FILE` deliberately takes a separate path: it scans the surrounding root,
then projects the requested file's discovery, risk, tests, graph adjacency, and findings into
an `ExplainReport` with focused renderers.

First-class (tree-sitter) languages: **Rust, Python, JavaScript, TypeScript/TSX, Go, PHP**.
Every recognized format contributes to complete inventory, token/context size, and line facts.
Health analysis defaults to programming/build source; HTML, CSS/SCSS, JSON, YAML, TOML, Markdown,
XML, and text require `health_includes` / `--health-include` or the explicit all-content health
scope. `health_excludes` / `--health-exclude` then remove repository-relative path globs from
complexity, markers, duplication, risk, test-presence, and cleanup signals while inventory,
tokens, lines, imports, symbols, and context discovery remain complete. The fixed order is scope,
then format includes, then path excludes; path excludes win. Generic code languages use heuristic
complexity flagged `approximate`; non-code formats do not receive complexity metrics.

Metric semantics worth knowing:

- **Line metrics are syntax-aware where grammars exist.** Rust, Python, JS/TS/TSX, Go, and PHP
  classify comment-only lines from tree-sitter comment ranges. Other formats use a quote-aware
  fallback and expose `line_metrics_approximate: true`; the summary counts these files in
  `line_metrics_approximate_files`.
- **Markers are comment-aware for first-class languages.** When a syntax tree is available,
  only comment nodes contribute TODO/FIXME/HACK occurrences; identifiers, strings, template
  strings, Python docstrings, and TSX text do not. Other source formats, explicitly opted-in
  content formats, and parse failures retain the raw-text fallback. Excluded content formats carry
  no per-file marker facts or canonical marker findings; marker health eligibility therefore
  participates in the per-file cache profile.
- **Complexity is per function, and only for code.** `summary.complexity.cyclomatic_*`
  / `cognitive_*` (avg, max, total) are computed over individual functions, not whole
  files. `max_complexity` / `--max-complexity` (default 20) is an ESLint-style reporting
  rule: `summary.complexity.functions_over_threshold` counts every function or method
  above the maximum, while `summary.complexity_violations` retains the worst top-N with
  `path`, `name`, and `line`. `summary.top_functions` remains the threshold-independent
  ranking, and per-file `complexity.functions[]` carries all callable detail. First-class
  callable scopes include named functions/methods plus JS arrows/function expressions,
  Rust closures, Python lambdas, Go function literals, and PHP closures/arrows; anonymous scopes inherit a
  binding name when possible and must not inflate their enclosing function.
  `--fail-on max-cyclomatic>N` therefore gates on the single worst function.
  Complexity runs only when `LangInfo::is_code()` is true and the path is health-eligible —
  prose/data/markup/style
  languages (Markdown, JSON, YAML, TOML, HTML, CSS, XML, …) get `complexity: null`
  (and `approximate: false`) and never enter the churn×complexity hotspot ranking.
  Heuristic (`approximate`) files are generic *code* languages (C, Java, …) with no
  bundled grammar; they contribute to `mi_avg` / `mi_min` but not to function-level
  cyclomatic/cognitive stats.
  Python comprehension clauses and JS/TS default values, logical assignments, optional
  chains, and nullish coalescing contribute control-flow paths. File-level cyclomatic values
  sum independent function scopes plus top-level decisions. Cognitive complexity includes
  direct self-recursion. Maintainability Index uses Microsoft's normalized `0..100` formula
  with SLOC as the cross-language source-operation proxy; `0..9` is low, `10..19` moderate,
  and `20..100` good.
  Halstead arithmetic follows the published equations, but grammar-specific leaf-token
  classification means those values are RepoScout-internal signals rather than cross-tool or
  cross-language equivalents.
- **Duplication is structured, format-scoped, similarity-scored, and line-filtered.**
  The zero-config corpus is source/build files only. `health_includes` adds selected content
  formats and `health_scope = "all"` restores every recognized format; `health_excludes` removes
  matching paths last. The equivalent CLI flags are `--health-include`, `--health-scope`, and
  `--health-exclude`. Inventory metrics are never filtered by this policy.
  `summary.duplication.duplicated_pct` uses `analyzed_lines`, not whole-repository LOC, so excluded
  content cannot dilute coverage. `duplicates.file_coverage` and `by_language` contain only
  eligible files/formats. The effective policy is recorded in `analysis_profile.health` and must
  match for baselines; reports without profile metadata are no longer baseline-compatible.
  Exact matching preserves structured token kinds/values; Type-2 candidate shapes
  normalize identifiers/literal categories, then verify every retained pair with a
  two-way identifier map. Exact detected formats are isolated by default; the opt-in
  `compatible` scope combines JS/TS/TSX. `mild` trivia filtering (ignore whitespace,
  retain comments) preserves historical behavior; `weak` also ignores comments. Every
  clone group (in `duplicates.exact` / `duplicates.near`) carries `format`, precise
  instance ranges, and `similarity`: exact clones are `1.0`; pair-oriented near groups
  are `< 1.0` and `>= near_dup_min_similarity`. `summary.top_duplicates` is the compact
  all-health-corpus rollup — the highest-impact blocks ranked by `duplicated_lines` (removable
  lines = `lines * (copies - 1)`), each with `copies`, `similarity`, and up to 10
  `locations` (`path:start-end`). The first block is retained; a later block must add at least
  `min_dup_lines` contiguous uncovered lines in at least two instances relative to already
  selected blocks. `summary.top_production_duplicates` applies the same compact filter after
  excluding test-only and direct Rust-inline-test-only families; table/Markdown use this
  production projection by default. A mixed production/test family remains visible. These are
  projections only: never delete or rewrite raw exact/near groups, coverage, canonical findings,
  or pair findings when changing their redundancy policy. Groups whose largest instance spans
  fewer than `min_dup_lines` lines (default 3) are dropped, so a single dense line that merely
  exceeds `min_dup_tokens` no longer shows up as a "clone". Instances that physically
  overlap another instance in the same file are pruned before grouping (a "copy" that
  overlaps another is not a separate copy), which removes tandem-repeat false positives
  — e.g. a sliding window over a block of structurally-identical lines such as a list
  of CSS custom properties. Note the detector matches duplicated *text*, so genuinely
  repeated-but-not-extractable blocks (e.g. identical per-file import preambles) can
  still rank; the `locations` make these easy to judge. `dup::DuplicateCoverage` is the
  single source of physical-line and duplication-token union semantics. Line/token
  percentages, per-language statistics, per-file coverage, and `--by-dir` therefore do
  not double-count exact/near overlap. The token denominator is the structured
  duplication lexer, never the tiktoken `summary.tokens` total. Full JSON also carries
  stable pair-oriented `duplicates.findings`; `summary.top_duplicate_findings` keeps a
  compact projection, and `--dup-details` expands table/Markdown output. Type-2 candidate
  discovery uses rolling rename-invariant fingerprints, schedules rare buckets before repetitive
  ones, tracks covered diagonals with merged interval lookup, and suppresses overlapping pairs as
  compact token ranges before constructing report objects. Each format pool is bounded to
  10,000,000 seed pairs, 250,000 buffered compact matches, and 10,000,000 suppression overlap
  checks. When a bound is reached, retain the useful partial groups but propagate the reason and
  omitted work through `Type2Diagnostics`, top-level `ScanDiagnostics`, human reports, capability
  discovery, and `type2_progress`; never present the resulting near-duplicate metrics as complete.
  Keep the early-reduction and deterministic rare-first properties when changing the detector,
  because repetitive large repositories otherwise create millions of temporary findings.
  Type-2 `similarity` is a weighted structured-token score: exact tokens receive `1.0`,
  consistently renamed identifiers `0.80`, and changed same-category literals `0.70`.
- **`--summary` is the agent scouting mode.** With `-f json --summary`, the heavy
  `files[]`, `duplicates`, and canonical finding arrays are dropped while the aggregate
  `summary` and explicitly requested `context`, `directories`, `graph`, `impact`, baseline, and
  review query blocks remain — a few KB instead of megabytes for an ordinary scan without
  erasing requested answers. The redundancy-filtered `summary.top_duplicates` and optional
  `summary.top_production_duplicates` are deliberately kept, so an agent still gets actionable
  duplication data. The table/markdown renderers use the production top-N rollup, so the flag
  mainly affects JSON. `--baseline-ready` removes opt-in analysis blocks.
  Summary JSON remains valid baseline input because baseline loading consumes only
  report metadata plus `summary` rather than requiring the omitted arrays. It cannot provide
  finding-level comparison. `--baseline-ready` is the finding-complete compact artifact: it
  also removes heavy arrays/opt-in analysis blocks but retains `finding_catalog`.
- **`--change-summary` is the bounded change-decision mode.** It requires exactly one of
  `--since`, `--staged`, or `--working`, defaults to the `agent` profile unless explicitly
  overridden, and implies context plus impact analysis. Its JSON/NDJSON contract identifies
  itself with `report_kind: "change-summary"` and retains only interpretation metadata,
  diagnostics, and the additive `change_summary` projection—never the ordinary aggregate,
  per-file facts, finding catalog, or raw context/impact blocks. Keep the aggregate 100-path,
  25-gap, and 10-validation limits synchronized with capability discovery and the bundled skill.
  Confidence must keep clean observed-scope evidence separate from repository-wide discovery
  blind spots; matching tests and validation entries are recommendations, never measured
  coverage or claims that a command ran.
- **Canonical findings are one shared contract.** `findings::build` projects every complexity
  violation, precisely located marker, duplicate family, and risk score >= 0.7 into the
  versioned top-level `finding_catalog`; it is not capped by `top`. Path-sensitive fingerprints
  use semantic function/marker identities, while duplication family fingerprints are
  content-derived and path-independent. Baseline and deep-review comparisons classify only
  `new`, `resolved`, `worsened`, and `improved`; new/worsened participate in regression gates.
- **Review uses Git content, not just changed filenames.** Bare `--review` filters current
  complexity/marker/duplication findings to zero-context changed-line ranges. `--review=deep`
  analyzes both snapshots through `scan::analyze_source`; staged current content comes from the
  index, other current content from the worktree, and base content from the selected ref/HEAD.
  Both snapshots honor current discovery policy. Duplication runs on each full snapshot so a
  changed fragment can match unchanged code. Git-detected renames are remapped before comparison.
  `--fail-on-review` gates all fast findings but only deep `new`/`worsened` states.
- **Output paths do not feed back into scans.** The CLI passes `-o/--output` as an exact
  filesystem exclusion to both scoped discovery and impact topology. Do not replace this
  with a glob: canonical identity prevents lookalike files from being skipped.
- **Caching never touches the scanned repo.** `cache.rs` stores results in the OS
  cache directory (`directories::ProjectDirs`) keyed by a hash of the canonical scan
  root, so scouting arbitrary repositories leaves no `.reposcout/` behind. Declaration outlines
  are cached independently of whether context output is requested. Graph source facts (import
  specifiers, parse diagnostics, and explicit type-relation declarations/references) enrich the
  same entry lazily for graph/context/impact/explain and do not alter the analysis profile. Do not
  extract them during ordinary scans or normal daemon refreshes. `reposcout cache clear [PATH]`
  removes both the analysis file and separate churn directory for that canonical scan root;
  `--all` removes the application cache directory. Keep reset idempotent, scoped by default, and
  explicit for the all-repository case.
- **Configuration is layered and inspectable.** Precedence is CLI flags > the nearest project
  `reposcout.toml`/`.reposcout.toml` > the OS-appropriate global `reposcout.toml` > defaults.
  Project/global files contain optional fields; only present fields override a lower layer, and
  nested `[context]` fields merge independently. Arrays replace lower-layer arrays, while CLI
  `--exclude` and `--health-include` values extend their effective lists. `REPOSCOUT_GLOBAL_CONFIG` is the explicit global
  path override used by hermetic automation and tests. `reposcout config [PATH]` reports source
  paths, loaded/ignored keys, precedence, and final file-configurable values. `--no-project-config`
  ignores repository-owned settings. The `agent` profile disables duplication/churn by default;
  `safe` additionally implies the project-config trust boundary and conservative worker/history/
  context/duplication plus discovery-policy guardrails, and forces source health scope with no
  content includes. Explicit analyzer selection may opt back
  into an analyzer under those settings; the profile does not claim a total runtime bound for an
  arbitrarily large target.
- **Agent task queries share scanner facts.** `reposcout capabilities -f json` performs no scan and
  describes commands, formats, profiles, limits, language coverage, and machine interfaces.
  `reposcout locate SYMBOL [PATH]` uses cached first-class declaration outlines with deterministic
  case-insensitive ranked matching or case-sensitive `--exact` matching, optional kind/language
  filters, and a hard 100-result cap. Its cold path intentionally performs the configured per-file
  analyzers (but not duplication/churn) to populate the ordinary scan cache; do not introduce a
  second query-only cache profile. Capability tests compare advertised commands with Clap, symbol
  kinds with parser-produced outlines, and language names with the canonical 31-format fixture
  matrix. `--error-format json` emits one structured stderr object for usage and runtime failures.
  Do not add a parallel parser/index/query implementation or an MCP dependency; the roadmap makes
  MCP an explicit non-goal and keeps stable CLI query contracts as the automation surface.
- **Lockfiles are skipped by default** (`exclude_lockfiles = true`; see the
  `LOCKFILES` list in `walk.rs`). `--include-lockfiles` (or `exclude_lockfiles =
  false`) re-includes them. Note that `.lock` / `.sum` files are unrecognized by
  `lang::detect` and skipped regardless; the exclusion mainly affects lockfiles with
  recognized extensions such as `package-lock.json` and `pnpm-lock.yaml`.
- **`.reposcoutignore`** is a reposcout-specific custom ignore file (gitignore syntax,
  per-directory, hierarchical) added to the `ignore` walker in `walk.rs`
  (`add_custom_ignore_filename`). It is honored regardless of `--no-ignore`, so it's the
  right place to exclude vendored/generated trees from scouting.
- **Scouting signals (all in `summary`, all agent-oriented).** `symbols` = aggregate
  function/type/export counts (first-class files only, from `metrics/symbols.rs`).
  `skip_candidates` = generated/minified/vendored files not worth reading, each with a
  `reason` (`metrics/classify.rs`); the same reason appears per-file as `skip_hint`.
  `test_presence` = test-vs-source split + matching-test estimate (`metrics/testcov.rs`;
  source/test keys retain package prefixes and nested logical directories so same-named
  files in separate packages do not cross-match;
  Rust `tests/cli.rs` conventionally matches the package `src/main.rs`, while inline
  `#[test]`/`#[cfg(test)]` counts a file as tested via per-file `has_inline_tests`). The
  serialized `untested_*` names are retained for compatibility;
  they mean "no matching test file or inline Rust test", not measured coverage.
  `top_risks` = source files ranked by risk algorithm `5`:
  `0.40·size + 0.40·complexity + 0.20·churn`, where each continuous factor is
  `value / (value + half_saturation_anchor)` and the anchors are 1,000 SLOC, cyclomatic 100,
  and 20 commits. Scores therefore remain monotonic above their former hard caps. Entries and
  file explanations carry `algorithm_version` plus raw SLOC/cyclomatic/churn inputs; ordering
  ties break on those inputs and then path. Filename-based test matching remains informational
  and never changes risk or cleanup scoring. `assessment` = the one-glance verdict (`fits_context`, `token_budget`,
  `cleanup_worth` ∈ {low,medium,high}, `reasons`); computed last in `aggregate()` from the
  other signals (`DEFAULT_CONTEXT_BUDGET = 200_000`); context fit uses `summary.source.tokens`,
  while `summary.tokens` remains complete inventory. Its `production_duplication` signal uses
  only non-test code files, excluding direct Rust `#[cfg(test)]` regions, and triggers above 15%.
  The evidence records the `production-source` corpus, duplicated/analyzed line counts,
  percentage, and `complete`; Type-2 truncation or source discovery/read limits make it partial.
  The percentage is a lower bound only when Type-2 work was the sole gap; omitted source files can
  change its denominator. Churn-only truncation does not change duplication completeness. Raw
  duplication summaries cover the effective health corpus. Repository-wide totals and
  `languages` remain complete inventory. Additive
  `source` totals and `top_source_token_files` drive concise human reports, whose language table
  collapses non-source formats into one content rollup.
  Top-level `diagnostics` records discovered/analyzed/unsupported/unreadable files, bounded
  unsupported-path examples, walker errors, and any bounded/partial Type-2 run (including skipped
  seed pairs/matches and which limit fired)
  so agents can tell whether an apparent absence is a scan gap or a lower-bound duplication result.
- **Work scope is raw bounded evidence, not agent routing.** Strategy `2` scan reports project
  primary inventory, production-source duplication evidence when available, focus/diff seeds,
  context selection and uncapped omission totals, observed
  dependents/tests, weak graph components, and primary/planning confidence into top-level
  `work_scope`. All path examples share the capability-advertised bound and component records are
  separately bounded; exact totals and omissions survive. The projection uses only analysis
  already requested by the caller, is retained by summary/change-summary/NDJSON output, and is
  removed from baseline-ready and graph-only formats. Graph components describe observed topology
  and never prove independent tasks or prescribe delegation.
- **Opt-in blocks are omitted unless their flag is passed.** `context` (`--context`, context
  budget/file flags, or `--focus`), `directories` (`--by-dir`), `baseline` (`--baseline`),
  `graph` (`--graph`), `impact` (`--impact`), and `review`
  (`--review`) each
  `skip_serializing_if` empty/None. Diff scope (`--since`/`--staged`/`--working`, via
  `git::changed_files` + `DiffScope`) filters the file set *before* analysis, so every
  aggregate reflects only the changeset. `--impact` and diff-seeded `--context` are deliberate
  topology/planning exceptions: both retain scoped metrics while consulting full-tree topology;
  context additionally consults full-tree cached per-file facts. A subpath target still scopes
  which changed files seed either mode; only the dependent/planning search expands to the repo.
- **Baselines are profile-compatible comparisons.** New reports carry an additive
  `analysis_profile` describing analyzer availability, diff scope, health-file policy,
  duplication settings, and finding settings.
  Baselines must match that profile, target scope, and effective token encoding; metrics from
  disabled analyzers are omitted. Finding comparison is complete only when both catalogs and
  finding profiles are compatible; older compatible reports without catalogs remain
  aggregate-only. Reports predating analyzer metadata are rejected because their health-file
  semantics cannot be established.
- **The dependency graph and impact analysis cover every first-class language heuristically.** `graph.rs`
  resolves relative imports, JSONC `tsconfig.json`/`jsconfig.json` `baseUrl` and `paths` (following
  local project references and relative extends with cycle protection), deterministic local
  `package.json` exports/imports/entrypoints/subpaths, JS-runtime extensions to checked-in TS,
  the fallback `@/` convention, and relative plus unambiguous repo-absolute/`src`-root Python
  forms, PHP namespace imports through nearest Composer `autoload` / `autoload-dev` PSR-4 and
  PSR-0 mappings, conventional PHP source roots, static include/require expressions, Rust external
  `mod` / `#[path]` declarations plus local `use` and Cargo library paths, and Go module/relative
  package imports from `go.mod`. Go package imports target a deterministic representative file;
  this is package-level evidence, not a claim of exact file references. Full
  graph output includes deterministic
  adjacency and edge records with resolver provenance; focus/direction/depth options project a
  bounded subgraph without rebuilding topology. Unresolved specifiers, syntax errors, invalid
  or ambiguous resolver configs, and unmatched focus paths remain diagnostics. Cycles come from an
  iterative Kosaraju SCC; orphans are `fan_in == 0` files that aren't entrypoints or tests.
  A separate additive symbol topology records only explicit `extends`, `implements`, trait, and
  embedding relationships found in first-class syntax. Qualified, same-file/scope, and globally
  unique short names may resolve; ambiguous names remain unresolved. Keep symbol edges distinct
  from import adjacency so file fan-in and type reach retain honest semantics.
  With a diff scope, `--impact` reports changed graph files plus direct/transitive unchanged
  importers and a conservative `high`/`partial`/`none` confidence.
- **The daemon graph is on demand.** Normal watched scans keep `cfg.graph = false`; opening the web
  dashboard's Graph tab calls `GET /api/graph?revision=N`, which builds from the latest completed
  report's file set away from the async runtime and caches one graph by report revision. The
  frontend builds deterministic mixed-language architecture scopes and performs bounded file-
  neighborhood projection locally. Architecture roots collapse redundant single-child scope
  chains into selectable parent groups whose useful immediate children are already visible;
  groups must not become empty intermediate drill-down screens. Single-click selection highlights
  immediate relationships; double-click enters a child scope or file neighborhood. When the
  selected file has syntax-proven symbol relations, the default file neighborhood is a pure
  browser-side semantic projection: keep the focus type central, separate incoming/outgoing
  `extends`/`implements`/embedding members into relationship parent groups, and show only bounded
  direct import context around them. It must remain language-agnostic over `symbol_edges`, preserve
  honest visible/total counts, and provide an explicit route back to the unrestricted
  direction/depth neighborhood. Explicit type reach takes precedence over import fan-in when
  sizing prominent type-bearing files. Graph navigation state must remain URL-addressable:
  `/graph/scope/...` identifies architecture scopes, `/graph/file/...` identifies focused files,
  and canonical query parameters retain non-default presentation, direction, and depth. Preserve
  breadcrumb navigation and browser Back/Forward behavior; do not turn transient single-click
  selection into history entries. Custom React Flow nodes must retain explicit dimensions so
  `onlyRenderVisibleElements`, initial fitting, and the minimap work before DOM measurement. Dense
  views intentionally use readable minimum zoom, subdued idle edges, and a high-contrast minimap.
  The browser renders at most 100 nodes. Do not move graph analysis into every daemon scan or
  remove that browser-side bound.
- **The context plan is a bounded ranking, not a source pack.** `context.rs` ranks existing
  `FileReport` facts under hard aggregate-token and file-count limits. Explicit focus paths add
  same-directory siblings, direct dependencies, direct/transitive first-class-language dependents, and
  matching tests; diff scopes automatically use changed paths as seeds. Direct configured/syntax
  evidence is distinct from heuristic/transitive confidence. Support files, entrypoints, graph
  centrality, risk, churn, and complexity provide general ranking signals. Selected first-class
  files receive cached body-free declaration headers under separate per-file/aggregate
  limits (16 symbols / 2 KiB per file, at most four private declarations, 16 KiB total); outlines
  never enter ordinary `files[]`. Focus resolves against both repository root and nested scan
  target, reports misses/ambiguity, and can retain an `outline_only` declaration projection when an
  explicit focus/change seed is too large for the source budget. Generated,
  minified, and vendored files are skipped unless focused or changed. The planner does no source/network I/O,
  and must remain deterministic and honest when graph coverage is unavailable. Deleted diff paths
  may be virtual graph seeds even though they have no `FileReport`; `planning_diagnostics` reports
  coverage of the separate full-tree universe while top-level diagnostics remain scoped.
- **SARIF / NDJSON / graph exports are pure renderers** (`report/sarif.rs`,
  `report/ndjson.rs`, `report/graph.rs`) over the
  same `ScanReport`. Without review, SARIF surfaces duplicate-code, high-complexity functions,
  and graph orphans. With review, it emits only review findings and maps deep states to SARIF
  baseline states. NDJSON emits summary metadata (including an optional context plan), files,
  duplicate pairs, and review findings,
  each tagged with `kind`; full NDJSON includes the requested graph in its summary record. DOT and
  Mermaid render the same graph projection without running external tools. Explain has a focused
  table/JSON/Markdown/NDJSON renderer and rejects SARIF/graph-only formats. Formats are selected
  via `-f` or inferred from the `-o` extension.

## Conventions

- Keep `cargo fmt` clean and `cargo clippy --all-targets -- -D warnings` passing.
- Update `CHANGELOG.md` in the same change for every user-visible addition, behavior
  change, fix, removal, compatibility change, or meaningful performance improvement.
  Keep `[Unreleased]` first, released versions newest first, and prepend new bullets at
  the top of their subsection. Pure refactors and test-only changes need no entry unless
  they materially affect users or supported behavior.
- Comment only where intent isn't obvious; avoid narrating the code.
- Prefer surgical changes; don't reformat or refactor unrelated code.
- CLI note: `args_conflicts_with_subcommands` is on, so global flags must come
  **after** a subcommand (git-style), e.g. `reposcout tokens --encoding cl100k_base src/`.

## Before you commit — validation checklist

1. Confirm `CHANGELOG.md` covers all notable user-visible/performance changes and keeps
   the newest entries at the top.
2. `cargo fmt --check`
3. `cargo clippy --all-targets -- -D warnings`
4. `cargo test` (all green; the harness is serialized by repository configuration).
5. `pnpm lint:frontend`
6. `pnpm build:web && pnpm test:web`
7. `pnpm build:landing`
8. `cargo build --release`  (refreshes the `reposcoutdev` symlink)
9. Sanity-run: `reposcoutdev -f json .` and confirm the output looks right.

## Testing notes

- Unit tests live beside their modules (`#[cfg(test)]`).
- Integration tests in `tests/cli.rs` run the compiled binary against
  `tests/fixtures/sample/`. Assertions target stable behavior (tokens, lines, languages, markers,
  output formats, `--fail-on`) so they survive analyzer changes. The shared command helper points
  `REPOSCOUT_GLOBAL_CONFIG` at `tests/fixtures/test-global.toml`, which isolates developer settings
  and caps each CLI child at two workers; precedence tests explicitly override it. The repository
  Cargo config serializes the test harness, and process-I/O tests use bounded synthetic trees.
- `tests/dup_languages.rs` consumes `tests/fixtures/dup_languages.toml` and requires
  actionable exact and Type-2 findings for every canonical `lang::detect` format through
  both frozen detector APIs and the CLI JSON contract. Keep its explicit 31-format set in
  sync whenever language support changes.
- The fixture tree intentionally contains a duplicated block and TODO/FIXME/HACK
  markers; keep those when editing fixtures or update the tests accordingly.
