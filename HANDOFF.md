# HANDOFF.md

A running handoff for the next agent picking up **reposcout**. Read this first for
*where the project is and why*, then the compact root `AGENTS.md` and every matching normative
reference it routes to under `docs/agents/` for *how to work in the repo*. Use `README.md` for
user-facing behavior.

_Last updated: 2026-09-04 · latest release 0.2.0 · JSON `SCHEMA_VERSION` 2.0 ·
`ANALYZER_VERSION` 16_

---

## North star (don't lose this)

reposcout exists to give **agents and humans a fast, consolidated status for a repo or
any path inside it**, so they can make decisions *before* diving in:

- **Does readable source fit in a context window?** → `summary.source.tokens` and
  `summary.assessment`; `summary.tokens` and `summary.files` remain complete inventory.
- **Give an agent only the first decision view.** → `--agent-summary` returns hard-bounded JSON
  with coverage, inventory, assessment, leading signals, and optional direct-versus-expansion
  context evidence. Seed/graph coverage and analyzer-specific partiality qualify empty results;
  use ordinary summary/full output for specialized detail blocks.
- **Just tell me the verdict.** → `summary.assessment` (`fits_context_known`, `fits_context`,
  `cleanup_worth_complete`, `cleanup_worth`, `unavailable_signals`, `reasons`) — the one-glance
  answer without treating a disabled analyzer as zero evidence.
- **Is it worth cleaning up?** → `summary.assessment.production_duplication`,
  `summary.top_production_duplicates`, `summary.complexity`, and `summary.top_functions`; use the
  raw all-health-corpus duplication fields only when that broader scope is intentional.
- **What should I read vs skip, and where's the risk?** → `summary.top_risks`,
  `summary.skip_candidates`, `summary.symbols`, `summary.test_presence`,
  `summary.top_hotspots`, `summary.top_token_files`.
- **What fits the task's actual budget, and why?** → `--context` plus repeatable
  `--focus`; top-level `context.files` is a hard-budgeted reading order with reasons and bounded
  body-free symbol outlines. A diff scope automatically seeds a change-aware plan.
- **What does this change require next?** → exactly one of `--since` / `--staged` / `--working`
  plus `--change-summary` returns a bounded decision report with reading order, known impact,
  matching-test evidence, confidence gaps, and validation categories rather than a generic health
  dump.
- **Where is a declaration, and what can this binary do?** → `reposcout locate SYMBOL [PATH]`
  and zero-scan `reposcout capabilities -f json`.
- **Need to invalidate persistent facts while debugging?** → `reposcout cache clear [PATH]`
  clears both analysis and Git-history caches for the canonical scan root; `--all` is explicit.
- **Why is a run slow, growing, or dead?** → repeat the same command with a fresh
  `--debug-log <FILE>`. Flushed NDJSON stages, per-file timing, two-second memory heartbeats, and
  Type-2 inner counters survive ordinary errors and most crashes.
- **How broad is the work actually observed to be?** → `work_scope`, `--by-dir`, change-summary,
  context, and impact expose bounded raw scope evidence without making delegation or routing
  decisions for the caller.
- **Need only one conditional machine decision?** → the bundled skill's progressive decision-query
  guidance combines the narrowest analyzer with one bounded, completeness-aware `jq` result
  instead of passing an entire report into agent context.

The design bias is therefore **high signal, low noise, machine-readable, fast**. When in
doubt, optimize for "an agent can trust and act on this in one glance" over completeness.

## Current state

- **Feature-complete for the core purpose, plus a full scouting/CI layer.** Tokens
  (tiktoken `o200k_base` default / `cl100k_base`), complexity (per-function
  cyclomatic/cognitive/nesting + Halstead + MI), duplication (exact + verified Type-2, ranked and
  tokenized in the configured worker pool), line metrics, markers, imports, git churn. Minified
  files and recognized JavaScript/CSS chunks remain inventory/navigation facts but are excluded
  from duplication by default; `--dup-include-artifacts` is the explicit opt-in. On top of that:
  per-file symbol counts, don't-read `skip_hint`s, test-presence, a composite per-file `top_risks`
  ranking, and a one-glance health `assessment`.
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
  cap keeps useful verified groups but marks Type-2 and production-duplication evidence partial in
  debug, JSON, table, and Markdown output. A percentage is a lower bound when skipped Type-2 work
  is the only gap; source omissions can move the complete-repository percentage either way.
  `capabilities` advertises the limits.
- **Scan modes.** `--by-dir[=DEPTH]` (per-directory rollup), `--since`/`--staged`/`--working`
  (diff-scoped scan), `--impact` (full-topology first-class-language reverse dependents for a diff),
  `--change-summary` (bounded change decision that defaults to the agent profile and implies
  context/impact), `--baseline <file>` + `--fail-on-regression` (compare vs a saved report, gate
  CI), and
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
  loaded keys, precedence, and effective values for humans or agents. Human reports end with a
  short configuration hint when no project configuration is active and distinguish built-in
  defaults from an already useful global configuration.
- **Automation profiles and trust boundaries.** `--profile agent` omits duplication/churn unless
  explicitly requested; `--profile safe` also ignores project config and applies conservative
  worker/history/context/duplication and discovery-policy guardrails;
  `--no-project-config` selects only the trust boundary. `--error-format json` makes failures
  structured, and `execution` records provenance, timings, cache behavior, and graph-fact coverage.
- **Trust signals.** Every report now carries top-level `diagnostics` (discovered/analyzed/
  unsupported/unreadable files, bounded unsupported-path examples, walker errors, and partial
  Type-2 reasons/omitted work); malformed config files fail loudly instead of silently falling
  back to defaults.
- **Security boundaries.** Scan/explain/locate output files use symlink-safe atomic replacement,
  including anchored Unix parent traversal. Release tags are validated, shell context crosses
  through environment variables, release commits must be reachable from `main`, and published
  assets receive attestations. The daemon is loopback-first and bearer-token authenticated;
  unauthenticated mode is loopback-only, while remote plain HTTP is explicit and intended only
  behind TLS.
- **Output:** `table` (human), `json` (agent), `markdown` (PRs/issues), `sarif` (SARIF
  2.1.0 for code scanning / CI), `ndjson` (streamable), and graph-only `dot` / `mermaid`.
  `--agent-summary` is the smallest general agent payload: JSON-only, fixed section caps, explicit
  projection omissions, and a hard 16 KiB document ceiling. `--summary` drops heavy arrays while
  retaining explicitly requested context/graph/directory/impact answers. Terminal tables share the detected width, shorten
  paths from the front, omit empty language rows, use semantic color, and place detailed inventory
  first so the overview, configuration hint, and most decision-relevant verdicts remain nearest
  the prompt.
- **Frontend:** `apps/web` is the Shadcn-based live daemon dashboard with an on-demand, searchable,
  bounded dependency-graph explorer. SSE reconnects reconcile the canonical snapshot and lagged
  streams reconnect instead of silently losing state. Daemon scans retain source facts and resolver
  configs for revision integrity, while graph topology is built and cached only when requested. The
  explorer groups mixed repositories into deterministic architecture scopes, supports breadcrumb
  drill-down and file neighborhoods, highlights direct relationships on one click, opens nodes on
  double-click, and provides factual scope/file/connection inspectors. Dense views retain readable
  zoom and subdued idle topology plus a populated high-contrast minimap; `apps/landing` is a
  standalone, responsive public product page on the same React/TypeScript/Vite/Tailwind stack,
  with bespoke styling and the RepoScout fox artwork.
- **Quality gates:** Rust formatting, clippy, and test suites; dashboard Vitest; and production
  builds for both frontend packages.
- **Development install:** `~/.local/bin/reposcoutdev` is a symlink to
  `target/release/reposcout`; `reposcout` is reserved for the public release.
  **Rebuild release after any code change** (`cargo build --release`) — see `AGENTS.md` and
  `docs/agents/validation.md`.
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
`report/agent_summary.rs` owns the hard-bounded pure scouting projection;
`debug_log.rs` owns the process-wide diagnostic session; `dup/fuzzy/plan.rs` owns deterministic
rare-first Type-2 admission; and `report::render` turns the `ScanReport` into table/json/markdown.

Analyzer signatures are decoupled and mostly frozen; `scan.rs` is their only caller.
Read `AGENTS.md` and `docs/agents/architecture-contracts.md` before changing any of them.

## Recent direction

RepoScout's core analyzers matured into a source-first, cache-backed scouting layer with bounded
context, change, graph, review, and machine-query contracts. The current focus is trustworthy
evidence and human/agent usability: explicit completeness, production-focused duplication,
guardrailed execution, safe outputs and releases, and concise terminal/JSON projections.

The detailed release history lives in `CHANGELOG.md`; do not rebuild it here. Keep only direction
that a new maintainer still needs to interpret the current architecture and roadmap.

## Known limitations & sharp edges

1. **Duplication matches text, not semantics.** `top_duplicates` is a strong cleanup
   signal, but a few entries can be real-yet-not-extractable (e.g. identical per-file
   import preambles). We deliberately do **not** try to classify "extractable vs not" —
   `similarity` + `copies` + `locations` let the reader judge in a second. Don't add a
   fragile heuristic here without strong evidence it beats the current honesty.
2. **Default CLI Type-2 analysis is bounded on pathological pools.** Reported matches retain the
   same verifier/precision, but recall is partial when `diagnostics.type2_analysis_partial` is
   true. Exact duplication remains complete. Combined percentages are lower bounds when skipped
   Type-2 work is the only gap; omitted source can also change the denominator. Do not remove
   limits or silently auto-escalate; a higher-effort mode should be reconsidered only if real usage
   shows that partial results materially reduce the tool's value.
3. **Vendored / generated source can still affect inventory and non-duplication health signals.**
   Minified files and recognized bundles/chunks are excluded from duplication by default, but all
   recognized content still affects inventory and token counts, and other generated or vendored
   source can still enter complexity, markers, risk, and test-presence. Use `health_excludes` /
   `--health-exclude` to retain such paths only as inventory/navigation facts, or `excludes` /
   `--exclude` to remove them from the complete scan. `.gitignore` is respected by default;
   lockfiles are excluded by default.
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
   imports, markers, and health-file eligibility/excludes). Declaration outlines enrich entries only
   for queries/context; graph import, parse, and type-relation facts enrich them lazily for CLI graph
   consumers and are captured by daemon refreshes to preserve immutable revision inputs.
   Churn has a separate OS-cache index of immutable commit events and exact result views.
   `Duplication`, graph topology, and `Summary` remain scan-wide computations. `--no-cache`
   disables both per-file and churn caches.
7. **`SCHEMA_VERSION` is `2.0`.** Test-presence output is optional and carries framework evidence,
   so consumers must treat an absent field as “no supported configured runner found.” Detection
   uses the bounded discovery universe for the requested scope plus fixed-name runner evidence in
   target ancestors through the Git root, reads candidate manifests through the bounded no-follow
   path, and scopes runner defaults to the evidence directory. The ancestor probe supplies project
   context without widening the analyzed file scope. Explain, rollups, baselines, risk output, and
   aggregate reports do not publish inferred source-to-test or `untested_*` claims. Additive fields
   (e.g. `similarity`, `top_duplicates`) have been treated as non-breaking for this pre-release tool.
   **Bump it and update the integration tests for any breaking JSON change.** Additive fields must
   carry `#[serde(default)]` so old baseline/cache JSON still deserializes before explicit
   compatibility checks.
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
   Context strategy `3` records explicit focus as structured high-confidence distance-zero
   evidence. Agent-summary classifies this and other direct evidence without changing the ordered
   underlying `context.files` contract; without a focus or change seed, it keeps the direct tier
   empty and presents general candidates only as bounded expansion. Oversized explicit seeds remain
   visible in the outline-only tier. Change-summary strategy `2` preserves focus evidence in the
   merged reading order.
11. **Configured ignore-file limits do not guard the ordinary full-profile discovery walk.** The
   main walker still lets the `ignore` crate load repository and Git ignore files directly; the
   bounded reader protects snapshot/explain paths, while the `safe` profile avoids the exposure by
   disabling repository-owned ignores. Do not claim `max_ignore_*` bounds apply to ordinary
   discovery until that path is changed.

## Candidate next steps (ideas, not commitments)

See `ROADMAP.md` for evidence-gated opportunities. None are release blockers or committed next
steps: higher-effort Type-2 analysis, additional team profiles, external diagnostic seeds,
historical trends, additional distribution channels, and optional precision-index interoperability
all require real usage evidence. MCP/read-only protocol work is explicitly out of scope; stable CLI
JSON/NDJSON and shared query contracts remain the integration baseline. The bar remains: does the
feature directly improve a scouting decision without making the default path slow or the evidence
less honest?

## Working agreements (the easy-to-forget ones)

- Fresh shells don't have cargo on PATH: `source "$HOME/.cargo/env"` first.
- **Rebuild `--release` after every code change** so the global symlink stays current.
- Global flags come **after** any subcommand (git-style; `args_conflicts_with_subcommands`).
- Add cross-cutting duplication policy in `dup/mod.rs` orchestration (currently
  `analyze_with_diagnostics`), not in the frozen `exact::detect` / `fuzzy::detect` adapters.
- Treat `HANDOFF.md` as a maintained current-state contract: update it in the same patch whenever
  architecture, defaults, trust boundaries, versions, limitations, or working agreements change;
  keep historical release detail in `CHANGELOG.md` instead.
- Do not edit skills.sh-managed `.agents/skills` files by hand. Edit only the canonical
  `skills/reposcout` package when changing RepoScout's own skill, then run the repository sync/check
  script to refresh its `.agents` mirror.
- For dependency alerts, trace the transitive path and exhaust the package manager's targeted
  lockfile/security update path before adding policy. Overrides, direct transitive pins, ignores,
  and release-age exclusions require proof, narrow scope, and an explicit removal condition;
  remove stale exceptions whose governing policy is inactive.
- Keep the root `AGENTS.md` as a compact mandatory router with essential safety and architecture
  invariants inline. Detailed normative guidance belongs in focused `docs/agents/` references;
  update routing and affected references together, and keep the root at or below roughly 9.6 KiB
  so the project-instruction ceiling retains headroom.
- Validation before commit covers Rust formatting, clippy, and tests; dashboard build/tests;
  the landing build; `cargo build --release`; and a repo-local sanity scan. The full proportional
  checklist is in `docs/agents/validation.md`.
