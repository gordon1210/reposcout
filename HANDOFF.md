# HANDOFF.md

A running handoff for the next agent picking up **reposcout**. Read this first for
*where the project is and why*, then `AGENTS.md` for *how to work in the repo*
(toolchain, layout, the frozen contract, validation checklist) and `README.md` for
user-facing behavior.

_Last updated: 2026-07-30 · version 0.1.8 · JSON `SCHEMA_VERSION` 1.0_

---

## North star (don't lose this)

reposcout exists to give **agents and humans a fast, consolidated status for a repo or
any path inside it**, so they can make decisions *before* diving in:

- **Does readable source fit in a context window?** → `summary.source.tokens` and
  `summary.assessment`; `summary.tokens` and `summary.files` remain complete inventory.
- **Just tell me the verdict.** → `summary.assessment` (`fits_context_known`, `fits_context`,
  `cleanup_worth_complete`, `cleanup_worth`, `unavailable_signals`, `reasons`) — the one-glance
  answer without treating a disabled analyzer as zero evidence.
- **Is it worth cleaning up?** → `summary.duplication.duplicated_pct`,
  `summary.top_duplicates`, `summary.complexity`, `summary.top_functions`.
- **What should I read vs skip, and where's the risk?** → `summary.top_risks`,
  `summary.skip_candidates`, `summary.symbols`, `summary.test_presence`,
  `summary.top_hotspots`, `summary.top_token_files`.
- **What fits the task's actual budget, and why?** → `--context` plus repeatable
  `--focus`; top-level `context.files` is a hard-budgeted reading order with reasons and bounded
  body-free symbol outlines. A diff scope automatically seeds a change-aware plan.
- **Where is a declaration, and what can this binary do?** → `reposcout locate SYMBOL [PATH]`
  and zero-scan `reposcout capabilities -f json`.
- **Need to invalidate persistent facts while debugging?** → `reposcout cache clear [PATH]`
  clears both analysis and Git-history caches for the canonical scan root; `--all` is explicit.
- **Why is a run slow, growing, or dead?** → repeat the same command with a fresh
  `--debug-log <FILE>`. Flushed NDJSON stages, per-file timing, two-second memory heartbeats, and
  Type-2 inner counters survive ordinary errors and most crashes.
- **Should I spawn a sub-agent for this subtree, or is it trivial?** → the summary as a
  whole, or `--by-dir` to compare subtrees in one run; `--since`/`--graph` to scope to a
  changeset or find dead code.

The design bias is therefore **high signal, low noise, machine-readable, fast**. When in
doubt, optimize for "an agent can trust and act on this in one glance" over completeness.

## Current state

- **Feature-complete for the core purpose, plus a full scouting/CI layer.** Tokens
  (tiktoken `o200k_base` default / `cl100k_base`), complexity (per-function
  cyclomatic/cognitive/nesting + Halstead + MI), duplication (exact + verified Type-2, ranked), line
  metrics, markers, imports, git churn. On top of that: per-file symbol counts,
  don't-read `skip_hint`s, test-presence, a composite per-file `top_risks` ranking, and a
  one-glance health `assessment`.
- **Source-first zero-config health.** Complete inventory, tokens/context size, and line facts
  retain every recognized format, but actionable health analysis defaults to programming/build source.
  HTML, CSS/SCSS, JSON, YAML, TOML, Markdown, XML, and text require repeated
  `--health-include` / `health_includes`, or explicit `--health-scope all` /
  `health_scope = "all"`. Repeatable `--health-exclude` / `health_excludes` path globs are applied
  last and retain inventory while removing complexity, markers, duplication, risk, test-presence,
  and cleanup signals. Human reports collapse content formats into one rollup and rank source
  files; duplication coverage denominators contain only eligible analyzed files.
- **Large-run observability and bounded Type-2 work.** Global `--debug-log` records flushed
  lifecycle/stage/file/render/error/panic events plus liveness/memory and detailed Type-2 progress.
  CLI duplication orchestration schedules rare fingerprint buckets first and caps each format pool
  at 10,000,000 seed pairs, 250,000 compact matches, and 10,000,000 suppression checks. Hitting a
  cap keeps useful verified groups but marks Type-2 results and combined duplication percentages as
  lower bounds in debug, JSON, table, and Markdown output. `capabilities` advertises the limits.
- **Scan modes.** `--by-dir[=DEPTH]` (per-directory rollup), `--since`/`--staged`/`--working`
  (diff-scoped scan), `--impact` (full-topology first-class-language reverse dependents for a diff),
  `--baseline <file>` + `--fail-on-regression` (compare vs a saved report, gate CI), and
  `--graph` (heuristic mixed-language import graph for every first-class language: stable
  adjacency/edges, fan-in/out, cycles, orphans, TS/JS config aliases/package metadata, Python
  relative/absolute imports, Composer namespace/static-include resolution, Rust modules/Cargo
  crates, and Go packages/modules).
  `--graph-focus` + direction/depth answer bounded dependency and blast-radius questions, while
  DOT/Mermaid export the same projection. The opt-in `--context` plan ranks focus paths, graph
  neighbors, matching tests, support files, risk, churn, and complexity under hard token/file
  limits, then attaches bounded structural outlines. With a diff scope it uses a separate
  full-tree planning universe for changed files/tests/dependents without widening report metrics.
  That universe has its own `context.planning_diagnostics`; top-level diagnostics stay scoped.
- **Layered team configuration.** Resolution is CLI > nearest project > global > defaults;
  project files override only fields they define. `reposcout config` exposes source paths,
  loaded keys, precedence, and effective values for humans or agents.
- **Automation profiles and trust boundaries.** `--profile agent` omits duplication/churn unless
  explicitly requested; `--profile safe` also ignores project config and applies conservative
  worker/history/context/duplication and discovery-policy guardrails;
  `--no-project-config` selects only the trust boundary. `--error-format json` makes failures
  structured, and `execution` records provenance, timings, cache behavior, and graph-fact coverage.
- **Trust signals.** Every report now carries top-level `diagnostics` (discovered/analyzed/
  unsupported/unreadable files, bounded unsupported-path examples, walker errors, and partial
  Type-2 reasons/omitted work); malformed config files fail loudly instead of silently falling
  back to defaults.
- **Output:** `table` (human), `json` (agent), `markdown` (PRs/issues), `sarif` (SARIF
  2.1.0 for code scanning / CI), `ndjson` (streamable), and graph-only `dot` / `mermaid`.
  `--summary` drops heavy arrays while retaining explicitly requested context/graph/directory/
  impact answers — the intended agent payload.
- **Frontend:** `apps/web` is the Shadcn-based live daemon dashboard with an on-demand, searchable,
  bounded dependency-graph explorer. It groups mixed repositories into deterministic architecture
  scopes, supports breadcrumb drill-down and file neighborhoods, highlights direct relationships
  on one click, opens nodes on double-click, and provides factual scope/file/connection inspectors.
  Dense views retain readable zoom and subdued idle topology plus a populated high-contrast
  minimap; `apps/landing` is a
  standalone, responsive public product page on the same React/TypeScript/Vite/Tailwind stack,
  with bespoke styling and the RepoScout fox artwork.
- **Quality gates:** Rust formatting, clippy, and test suites; dashboard Vitest; and production
  builds for both frontend packages.
- **Development install:** `~/.local/bin/reposcoutdev` is a symlink to
  `target/release/reposcout`; `reposcout` is reserved for the public release.
  **Rebuild release after any change** (`cargo build --release`) — see AGENTS.md.
- **CI gates:** `--fail-on "max-cyclomatic>30,duplicated-pct>5,…"` (exit 2), or
  `--baseline b.json --fail-on-regression` to fail when any metric worsens.

## Mental model (60 seconds)

`src/model.rs` is the **serde contract**: every analyzer writes into these structs,
every reporter reads from them, integration tests assert the shape. `config.rs` resolves
global/project layers before `main.rs` applies CLI overrides. `scan.rs`
orchestrates: discover files (`walk`), analyze each in parallel (`rayon`), consult the
`cache`, run cross-file duplication, attach `git` churn, aggregate a `Summary`, and optionally
derive reusable graph analyses plus the bounded plan in `context.rs`. Diff-scoped context keeps
the primary analysis scoped and builds a distinct cached planning universe; `graph.rs` owns the
shared topology used by both context and impact.
`query.rs` owns task-oriented capabilities/symbol lookup over the same cached facts;
`debug_log.rs` owns the process-wide diagnostic session; `dup/fuzzy/plan.rs` owns deterministic
rare-first Type-2 admission; and `report::render` turns the `ScanReport` into table/json/markdown.

Analyzer signatures are decoupled and mostly frozen; `scan.rs` is their only caller.
See the "frozen contract" section in AGENTS.md before changing any of them.

## How we got here (recent arc, newest first)

The tool was built as a skeleton + analyzers, then **hardened by dogfooding on
real-world repositories** (a large TypeScript/React codebase among them). Each fix below
came from "I ran it and the output wasn't actually useful":

**Feature expansion (newest first).** Implemented in scoped waves, then reviewed,
validated, and sanity-run before committing. All changes remain additive (`SCHEMA_VERSION` is
still `1.0`); `ANALYZER_VERSION` is now `15`.

- **2026-07-17 — source-first default health corpus.** `reposcout .` now keeps complete
  repository inventory and context-size evidence without feeding data, docs, markup, styles, or
  text into marker and duplication health results. One centralized language policy partitions the
  22 default program/build formats from 9 explicit content formats. CLI/config opt-ins select one
  or all content formats; safe mode forces source-only. Profiles, cache identity, baseline
  compatibility, capability discovery, JSON source rollups, and eligible line/token denominators
  all record the same policy. Baselines without profile metadata are intentionally rejected because
  their historical all-content semantics are unknowable.

- **2026-07-17 — agent-efficient CLI contract.** Added agent/safe execution profiles,
  repository-config opt-out, capability discovery, bounded cross-language symbol lookup,
  structured JSON errors, execution/cache telemetry, honest partial assessments, explicit focus
  resolution, and outline-only oversized focus results. Compact output retains requested query
  blocks. Declaration outlines no longer split cache profiles; graph source facts enrich the same
  per-file cache only on demand and are reused by graph/context/impact/explain. Exact symbol lookup
  is case-sensitive; a cold lookup performs configured per-file analysis and then reuses that same
  cache. MCP is explicitly out of scope; stable CLI JSON/NDJSON and shared query contracts are the
  supported automation surface.

- **2026-07-17 — live debug diagnostics + pathological Type-2 guardrails.** Added the global,
  no-overwrite `--debug-log` NDJSON trace with exact scan/watcher exclusion, per-stage/file timing,
  runtime/panic records, two-second memory heartbeats, and detailed rate-limited Type-2 phases.
  A real large-repository trace exposed 832,934,702 JSON seed pairs and multi-gigabyte linear
  covered-region/match growth. Covered diagonals now use merged interval lookup; bounded CLI
  orchestration admits rare buckets first and caps seed pairs, compact matches, and suppression
  comparisons per format pool. Partial near-duplicate output is explicitly a lower bound in every
  reporter and capability discovery. The public frozen detector adapter stays exhaustive.

- **2026-07-16 — structural/change-aware context + graph precision.** Context strategy v2 adds
  bounded body-free declarations, reasons, serialized payload/omission measurements, and planning
  time. Diff scopes seed a separate full-tree plan with structured evidence for changes,
  dependencies, tests, nearby code, and direct/transitive dependents; deleted paths remain virtual
  graph seeds, scoped metrics stay scoped, and impact reuses the same topology. Graph resolution
  adds deterministic local package exports/imports/entrypoints, TypeScript source substitution,
  and unambiguous Python absolute/`src` imports. SCIP remains a documented future opt-in input,
  not a hidden indexing dependency.

- **2026-07-16 — all-language hierarchical graph explorer.** Rust external modules, local use
  paths, and Cargo library names plus Go module/package imports now join JS/TS/TSX, Python, and PHP
  in one mixed-language topology. Go imports deliberately target a stable package representative.
  The dashboard now opens on architecture scopes with weighted aggregate connections, smooth
  Bézier routing, breadcrumbs, scope drill-down, file neighborhoods, all-report-file search,
  direct-relationship single-click selection, deliberate double-click navigation, dense-view
  decluttering, a reliable minimap, and detailed factual scope/file/connection inspectors.
  Type-bearing files with explicit relations open into a centered semantic neighborhood with
  labeled incoming/outgoing type groups, quiet bounded direct-import context, honest truncation
  counts, and an escape to the full direction/depth neighborhood. The projection is shared by all
  first-class languages rather than keyed to language names. Architecture scopes and file
  neighborhoods are also routed under `/graph/scope/...` and `/graph/file/...`; presentation,
  direction, and depth round-trip through canonical query parameters, while breadcrumbs and browser
  history remain the navigation surface. It has no AI, tour, or source-opening action and retains
  the 100-node render bound.

- **2026-07-16 — on-demand web graph foundation.** The dashboard Graph tab lazily calls
  `GET /api/graph` for the current report revision; ordinary daemon scans remain graph-free. The
  daemon computes once per revision, while the browser provides a Dagre-laid-out React Flow canvas,
  file search, dependencies/blast radius/both-direction traversal, one-to-three-hop controls,
  resolver-aware details, a minimap, and a hard 100-node render cap.

- **2026-07-15 — queryable repository graph.** Graph JSON/NDJSON now exposes deterministic
  adjacency and edge records with resolver provenance. Bounded focus/direction/depth queries answer
  direct dependency and blast-radius questions, DOT/Mermaid provide external-tool-free exports,
  and JSONC `tsconfig`/`jsconfig` `baseUrl` + `paths` mappings (including local references and
  relative extends) improve TypeScript resolution while invalid config remains an explicit
  diagnostic that lowers impact confidence.

- **2026-07-15 — layered config + agent context plan.** Global personal defaults now compose
  with the nearest team-committed project config and CLI overrides, with field-level nested
  merging and an inspectable `reposcout config` command. `--context` produces an explainable,
  deterministic reading list under hard token/file caps, optionally centered on repeated
  `--focus` paths, same-directory siblings, and their direct first-class-language graph neighborhood. It
  reuses normal scan facts, remains opt-in, and is retained in summary JSON/NDJSON. The research
  and next priorities live in `ROADMAP.md`. The same validation pass fixed `explain` mistaking
  macOS's system `/var` alias for a repository-internal symlink and accidentally scanning from
  `/`.

- **2026-07-09 — correctness + trust + impact pass.** Cache validity is now keyed by the
  effective per-file analysis profile, so focused scans cannot poison full scans. Discovery
  owns stable report/cache paths (including standalone files) and reports walker errors;
  scan diagnostics expose unsupported/unreadable outcomes. `DuplicateCoverage` gives global
  and directory rollups the same physical-line semantics. `--impact` builds full first-class-language
  topology for a diff-scoped scan and reports direct/transitive unchanged importers. Invalid
  config now fails loudly; generated-header detection no longer mistakes explanatory prose for
  generated source.

- **Wave 6 — SARIF + NDJSON output + `.reposcoutignore`.** `-f sarif` (SARIF
  2.1.0: duplicate-code, high-complexity-function, orphan-file results), `-f ndjson`
  (summary line + one line per file, each `kind`-tagged), and a custom ignore file honored
  even with `--no-ignore`.
- **Wave 5 — dependency graph (`--graph`).** Self-contained `graph.rs`; initially
  resolved relative/`@/` (JS/TS) and dotted-relative (Python) imports to scanned files and reported
  fan-in/out, cycles (Kosaraju SCC), and orphan (dead-code) files. Later waves expanded it to every
  first-class language.
- **Wave 4 — diff-scoped scan + baseline compare.** `--since`/`--staged`/
  `--working` narrow to a git diff; `--baseline` + `--fail-on-regression` diff against a
  saved report and gate CI (exit 2).
- **Wave 3 — per-directory rollup (`--by-dir[=DEPTH]`).** `directories[]` with
  per-subtree tokens/SLOC/complexity/dup/untested counts + a "By directory" table.
- **Wave 2 — test-presence + composite risk + assessment.** `summary.test_presence`,
  `summary.top_risks` (size×complexity×churn, untested penalty), and the `summary.assessment`
  verdict.
- **Wave 1 — per-file symbols + don't-read flags.** `summary.symbols` /
  per-file `symbols`, and `skip_candidates` / per-file `skip_hint` for
  generated/minified/vendored files. (Two review fixes: whole-segment vendored matching;
  symbols piggyback on the existing tree parse instead of forcing an extra AST parse.)

Earlier, driven by dogfooding:

- **Prune overlapping instances from clone groups.** A block of
  structurally-identical lines (e.g. a long list of CSS custom properties) produced a
  "clone group" whose copies were overlapping sliding windows (lines 18-22, 19-23,
  20-24, …) counted as many separate instances. A copy that physically overlaps another
  in the same file is *not* a distinct copy. We now greedily keep non-overlapping
  instances (by `path`, `start_line`) and drop groups left with < 2 copies. This removed
  a whole class of false positives while leaving genuine cross-file clones untouched.
- **Make duplication output actionable.** Reports previously showed only group
  *counts*. Added: `similarity` on every clone group (exact = 1.0; near = lowest pairwise
  similarity in the group); `min_dup_lines` (default 3) to drop dense single-line "clones";
  and `summary.top_duplicates` — the highest-impact blocks ranked by removable lines
  (`lines * (copies - 1)`) with `copies`, `similarity`, and `locations`. Rendered as a
  "Top duplicates" section in table/markdown. Kept in `--summary` on purpose.
- **Agent scouting mode.** `--summary` (drop `files[]`/`duplicates`, ~MB → ~KB);
  complexity computed **only for real code** (`LangInfo::is_code()`), so prose/data/markup
  no longer get bogus cyclomatic scores; hotspots restricted to code files; **cache moved
  to the OS cache dir** so scanning never writes into the target repo.
- **Lockfiles excluded by default + per-function complexity** (`top_functions`,
  "Most complex functions" section).
- **Duplication metric fixed** to a bounded line union (no more double-counting).

## Known limitations & sharp edges

1. **Duplication matches text, not semantics.** `top_duplicates` is a strong cleanup
   signal, but a few entries can be real-yet-not-extractable (e.g. identical per-file
   import preambles). We deliberately do **not** try to classify "extractable vs not" —
   `similarity` + `copies` + `locations` let the reader judge in a second. Don't add a
   fragile heuristic here without strong evidence it beats the current honesty.
2. **Default CLI Type-2 analysis is bounded on pathological pools.** Reported matches retain the
   same verifier/precision, but recall and combined duplication percentages are lower bounds when
   `diagnostics.type2_analysis_partial` is true. Exact duplication remains complete. Do not remove
   limits or silently auto-escalate; a higher-effort mode should be reconsidered only if real usage
   shows that partial results materially reduce the tool's value.
3. **Vendored / generated source can still inflate health signals; all recognized content still
   affects inventory and token counts.** Bundled UI component libraries (e.g. shadcn/ui), `dist/`,
   and codegen output can be removed only from health analysis with `health_excludes` or
   `--health-exclude`, or removed from the complete scan with `excludes` / `--exclude`.
   `.gitignore` is respected by default; lockfiles are excluded by default.
4. **First-class complexity is limited to Rust, Python, JS, TS/TSX, Go, and PHP.** Generic code
   languages get tokens/lines/markers/dup plus *heuristic* complexity flagged `approximate`
   (contributes to `mi_avg`/`mi_min` but not to per-function cyclomatic/cognitive stats).
   Non-source formats always retain inventory/line facts and receive health analysis only when
   their format or all-content scope is explicitly selected.
5. **Heuristic thresholds.** `min_dup_tokens=50`, `min_dup_lines=3`,
   `near_dup_min_similarity=0.85` are defaults that worked well in practice, tunable via
   `reposcout.toml`. There's no magic here — revisit them if a repo type shows noise.
6. **Per-file cache entries store `FileReport` plus reusable source facts**, keyed by content hash
   + version + the effective per-file analysis profile (token encoding/enablement, complexity,
   imports, markers, and health-file eligibility/excludes). Declaration outlines enrich entries only for queries/context; graph import,
   parse, and type-relation facts enrich them only when a graph consumer requests them.
   Churn has a separate OS-cache index of immutable commit events and exact result views.
   `Duplication`, graph topology, and `Summary` remain scan-wide computations. `--no-cache`
   disables both per-file and churn caches.
7. **`SCHEMA_VERSION` is still `1.0`.** Additive fields (e.g. `similarity`,
   `top_duplicates`) have been treated as non-breaking for this pre-release tool. **Bump
   it and update the integration tests for any breaking JSON change.** Additive fields must
   carry `#[serde(default)]` so old baseline/cache JSON still deserializes.
8. **The dependency graph and `--impact` cover every first-class language heuristically.** `graph.rs`
   resolves relative imports, TypeScript `baseUrl`/`paths`, deterministic local package metadata,
   unambiguous Python absolute/`src` imports, Composer PSR-4/PSR-0 maps, and static PHP includes,
   plus Rust module/Cargo-local paths and Go module-local package imports. It is not a compiler,
   package manager, or SCIP/language-server index. Custom package conditions, external packages,
   symbol-level Rust references, and exact intra-package Go file references remain outside the
   graph. Each edge names its resolver; unresolved imports, parse/config errors,
   and impact confidence make uncertainty explicit. Treat orphans, cycles, and impact as strong
   hints, not proofs.
9. **`ANALYZER_VERSION` (cache.rs) gates the cache, not `SCHEMA_VERSION`.** Any change to
   what a `FileReport` contains needs an `ANALYZER_VERSION` bump or stale cache entries
   will resurface. Summary/top-level-only changes don't. `--no-cache` sidesteps it.
10. **The context plan ranks whole files and outlines declarations; it does not pack source
   fragments.** Every first-class language can contribute graph relationships; other recognized
   languages retain non-graph ranking signals. It deliberately omits a file that cannot fit the hard token/file
   budget but retains a bounded `outline_only` projection for oversized explicit focus/change
   seeds, and independently caps outline symbols/bytes. Focus misses/ambiguity remain explicit. In diff mode, `context` may reference
   unchanged full-tree files even though `summary`, `files`, and findings remain diff-scoped.

## Candidate next steps (ideas, not commitments)

See `ROADMAP.md` for post-0.1 evidence-gated opportunities. None are release blockers or committed
next steps: higher-effort Type-2 analysis, reusable team profiles, historical trends, and optional
precision-index interoperability should be considered only when real usage demonstrates a clear
need. MCP/read-only protocol work is explicitly out of scope; stable CLI JSON/NDJSON and shared
query contracts remain the integration baseline. The bar remains: does the feature directly
improve a scouting decision without making the default path slow or the evidence less honest?

## Working agreements (the easy-to-forget ones)

- Fresh shells don't have cargo on PATH: `source "$HOME/.cargo/env"` first.
- **Rebuild `--release` after every code change** so the global symlink stays current.
- Global flags come **after** any subcommand (git-style; `args_conflicts_with_subcommands`).
- Add cross-cutting duplication policy in `dup/mod.rs` orchestration (currently
  `analyze_with_diagnostics`), not in the frozen `exact::detect` / `fuzzy::detect` adapters.
- Validation before commit covers Rust formatting, clippy, and tests; dashboard build/tests;
  the landing build; `cargo build --release`; and a repo-local sanity scan. (Full checklist in
  AGENTS.md.)
- Commits include the `Co-authored-by: Copilot <223556219+Copilot@users.noreply.github.com>`
  trailer.
