<p align="center">
  <img src="apps/web/src/assets/reposcout.png" alt="RepoScout" width="640">
</p>

# reposcout

> Fast repository scout — tokens, complexity, duplication & health metrics for a repo or any path inside it.

`reposcout` is a Rust CLI that scans a git repository (or a subdirectory / single
file) and prints a **consolidated status** of the code in seconds. It is built for
two audiences:

- **Agents** — stable, machine-readable JSON to quickly understand a codebase.
- **Humans** — a compact, colored terminal summary (or Markdown for PRs/issues).

See [CHANGELOG.md](CHANGELOG.md) for changes, listed newest first, and
[ROADMAP.md](ROADMAP.md) for the competitive research and agent-first direction.

## Features

- **Token counting** via [`tiktoken`](https://github.com/openai/tiktoken) encodings —
  `o200k_base` (default) and `cl100k_base`, configurable.
- **Function complexity rule** — like ESLint's
  [`complexity`](https://eslint.org/docs/latest/rules/complexity) rule, reposcout flags
  each AST-supported function, method, closure, lambda, or function literal whose
  cyclomatic complexity exceeds a configurable maximum (`--max-complexity`, default
  `20`). Reports include the symbol name, precise location, limit, and excess. Cognitive
  complexity and nesting are also shown per callable; per-file complexity and
  Maintainability Index remain available as supporting context.
- **Duplication** — source-first, format-scoped exact clones plus Type-2 clones with a verified
  one-to-one identifier rename map. A structured lexer distinguishes identifiers,
  literal categories, operators, punctuation, and comments while retaining precise
  line/column/byte/token ranges. Data, documentation, markup, and style formats remain in
  repository inventory but enter marker/duplication health analysis only through an explicit
  opt-in. Clone groups stay compact; pair findings, union-based line/token coverage, and
  per-language coverage make each result actionable.
- **Line metrics** — LOC / SLOC, comment lines & comment ratio, per-language breakdown.
- **Health signals** — source-scoped, comment-aware TODO/FIXME/HACK markers for first-class
  languages (raw-text fallback for other opted-in formats), largest source files by tokens,
  import/dependency extraction,
  and git churn hotspots (`churn × complexity`).
- **Scouting signals** — per-file symbol counts (functions/types/exports), "don't-read"
  skip hints for generated/minified/vendored files, a test-presence estimate, a composite
  per-file **risk** ranking (size × complexity × churn, modestly adjusted when no matching
  test file or inline Rust test is found), and a
  one-glance health **assessment** (is it worth cleaning? does it fit a context window?).
- **Agent context plan** — opt-in `--context` turns those scan facts into a deterministic,
  explainable reading list under hard token and file budgets. Selected first-class-language files
  carry bounded, body-free symbol outlines with signatures and reasons. Repeatable `--focus`
  paths prioritize the requested code and its graph/test neighborhood; a diff scope instead seeds
  changed files, matching tests, dependencies, direct/transitive dependents, and nearby risk while
  keeping the ordinary report diff-scoped.
- **Structure & change** — a per-directory rollup (`--by-dir[=DEPTH]`), diff-scoped scans
  (`--since <ref>` / `--staged` / `--working`), changed-line review (`--review` or
  `--review=deep`) with CI gating, finding-level baseline comparison, and an AST-backed
  **dependency graph** (`--graph`) with import fan-in/out, explicit type relationships, import
  cycles, and orphan (dead-code candidate)
  detection for every first-class language, including mixed-language repositories. It exposes
  deterministic adjacency/edges, understands Rust modules and Cargo-local crates, Go modules,
  `tsconfig.json` / `jsconfig.json` aliases, local `package.json` export/import maps and
  entrypoints, Python absolute imports through conventional `src/` roots, Composer PSR-4/PSR-0
  autoload mappings, and static PHP includes/requires. Explicit class, interface, trait, and
  embedding relationships are resolved conservatively and remain distinct from imports. It supports bounded
  dependency/dependent queries, and exports DOT or Mermaid. Diff-aware `--impact` reports direct
  and transitive internal dependents of a change set.
- **Explain one file** — `reposcout explain FILE` scans the surrounding repository, then
  projects discovery/ignore provenance, metrics, risk factors, test matches, direct graph
  neighbors, and every related finding onto that file.
- **Agent query contract** — `reposcout capabilities -f json` advertises commands, formats,
  profiles, bounds, and language coverage without scanning. `reposcout locate SYMBOL [PATH]`
  performs bounded, deterministic declaration lookup across all first-class languages. A cold
  lookup performs the configured per-file analysis while discovering declarations; later lookups
  reuse the same ordinary scan cache. The `agent` profile skips duplication and churn unless
  explicitly requested; the `safe` profile additionally ignores repository-owned configuration
  and applies conservative worker, history, context, discovery-policy, and duplication guardrails.
  `--error-format json` makes usage and runtime failures parseable, while report execution
  metadata exposes configuration provenance, stage timings, cache behavior, and graph-fact
  coverage.
- **Debug diagnostics** — opt-in `--debug-log <FILE>` writes immediately flushed,
  schema-versioned NDJSON for the invocation, effective configuration, discovery totals,
  top-level and detailed stages, per-file worker timings, rendering/output, runtime errors, and
  Rust panics with backtraces. Two-second heartbeats include memory usage, while Type-2 progress
  identifies the active format pool, bounded work, throughput, and any safety limit reached. The
  fresh log file is excluded from the scan and daemon watcher so diagnostics cannot feed back into
  results or refreshes.
- **Live daemon and web UI** — `reposcout daemon` watches a target, keeps the latest successful
  report available over HTTP, and publishes scan lifecycle events over SSE. The React dashboard
  presents live repository, language, risk, complexity, file, and finding views. Its on-demand
  graph groups mixed repositories into drillable architecture scopes, then exposes bounded file
  neighborhoods plus factual scope/file/connection inspectors with resolver provenance.
- **Scan completeness** — every JSON report includes discovered/analyzed/unsupported/unreadable
  counts plus walker errors. If repetitive input reaches a built-in Type-2 safety bound, the same
  diagnostics explicitly mark near-duplicate analysis as partial and quantify omitted work.
- **Output formats** — human `table`, agent `json`, `markdown`, `sarif` (SARIF 2.1.0 for
  code-scanning / CI), `ndjson` (streamable, one record per line), plus graph-only `dot` and
  `mermaid` exports.
- **Fast** — parallel scanning with [`rayon`](https://github.com/rayon-rs/rayon), a batched native
  Git history stream, incremental per-file and Git-history caches, and interactive progress that
  transitions from the file bar into elapsed-time stage feedback for git history, exact/Type-2
  duplication, caching, aggregation, graph, and impact work. `--quiet` suppresses all progress
  feedback.
- **`.gitignore`-aware** file discovery via the [`ignore`](https://github.com/BurntSushi/ripgrep/tree/master/crates/ignore) crate.
  Dependency lockfiles (`Cargo.lock`, `package-lock.json`, `yarn.lock`, `go.sum`, …) are
  skipped by default; pass `--include-lockfiles` to scan them. A repo-local
  `.reposcoutignore` (gitignore syntax, per-directory) excludes paths from scouting — handy
  for vendored/generated trees — and is honored even with `--no-ignore`.
- **Team-ready configuration** — built-in defaults are layered with an OS-appropriate global
  config, the nearest project `reposcout.toml`, and CLI flags. `reposcout config` shows the
  source files, explicitly set keys, precedence, and final effective values.

First-class (AST-based) languages: **Rust, Python, JavaScript, TypeScript/TSX, Go, PHP**.
Every recognized format still contributes to complete repository inventory, token/context size,
and line metrics. By default, marker and duplication health analysis covers programming languages,
SQL, Dockerfiles, and Makefiles. HTML, CSS/SCSS, JSON, YAML, TOML, Markdown, XML, and text are
explicit opt-ins via `--health-include <FORMAT>` or `health_includes`; use
`--health-scope all` / `health_scope = "all"` for the historical all-content corpus.

## Install

Build from source (requires a Rust toolchain and a C compiler for the vendored
tree-sitter grammars and libgit2):

```sh
cargo build --release
# binary at ./target/release/reposcout
```

Install the RepoScout agent skill from this repository with the
[skills CLI](https://skills.sh/docs/cli):

```sh
npx skills add gordon1210/reposcout --skill reposcout
```

The skill teaches supported coding agents to use RepoScout's compact JSON scouting,
context-planning, symbol-query, graph, impact, and review workflows. It expects the
`reposcout` binary above to be available on `PATH`.

## Usage

```sh
reposcout [PATH]            # full scan of PATH (defaults to ".")
```

`PATH` may be a repo root, a subdirectory, or a single file. The git root is
auto-detected so churn works even when scanning a subpath.

### Focused subcommands

Each subcommand runs only the relevant analyzers:

```sh
reposcout tokens     [PATH]   # token counts only
reposcout complexity [PATH]   # complexity metrics only
reposcout dup        [PATH]   # duplication only
reposcout churn      [PATH]   # git churn / hotspots only
reposcout metrics    [PATH]   # tokens + markers + imports
reposcout explain    FILE     # full-repository context for one file
reposcout locate     SYMBOL [PATH] # ranked/exact declaration lookup
reposcout cache clear [PATH]  # clear analysis + Git-history caches for one scan root
reposcout config     [PATH]   # inspect layered config sources and effective values
reposcout capabilities         # machine-readable feature discovery
reposcout daemon     [PATH]   # watch PATH and serve live scan results
```

> **Note:** because subcommands take their own options, global flags must come
> *after* the subcommand, e.g. `reposcout tokens --encoding cl100k_base src/`.

`locate` uses case-insensitive ranked matching by default (qualified exact, simple exact,
prefix, then substring). `--exact` restricts it to a case-sensitive qualified or simple full-name
match; add `--kind <KIND>`, `--language <LANGUAGE>`, or `--limit <1..100>` to narrow it further.
It supports table, JSON, Markdown, and NDJSON output. A cold lookup walks the target and performs
the configured per-file analyzers, excluding whole-corpus duplication and churn; subsequent
lookups reuse cached file reports and declaration outlines.

### Options

| Flag | Description | Default |
|------|-------------|---------|
| `-f, --format <table\|json\|markdown\|sarif\|ndjson\|dot\|mermaid>` | Output format; DOT/Mermaid render only the requested graph | `table` on a TTY, else `json` |
| `-o, --output <FILE>` | Write to a file (also infers `.md`, `.sarif`, `.ndjson`/`.jsonl`, `.dot`/`.gv`, `.mmd`/`.mermaid`); its exact path is excluded from the scan | stdout |
| `--profile <full\|agent\|safe>` | `agent` omits duplication/churn by default; `safe` also ignores project config and applies conservative guardrails | `full` |
| `--no-project-config` | Ignore the nearest repository-owned `reposcout.toml` while retaining user/global settings | off |
| `--error-format <text\|json>` | Render usage/runtime failures as text or one JSON object on stderr | `text` |
| `--debug-log <FILE>` | Write flushed NDJSON diagnostics for slow or crashing runs; refuses to overwrite an existing file | off |
| `--only <a,b,...>` | Restrict analyzers; cannot be combined with an analyzer subcommand | all |
| `--exclude <GLOB>` | Extra ignore glob (repeatable; extends config-file excludes) | — |
| `--include-lockfiles` | Scan dependency lockfiles (off by default) | off |
| `--encoding <NAME>` | `o200k_base` or `cl100k_base` | `o200k_base` |
| `--hidden` | Include hidden files | off |
| `--no-ignore` | Do not respect `.gitignore` (`.reposcoutignore` still applies) | off |
| `-j, --jobs <N>` | Worker threads | CPU count |
| `--no-cache` | Disable the incremental cache | off |
| `--top <N>` | Length of "top N" lists | 10 |
| `--max-complexity <N>` | Flag functions/methods with cyclomatic complexity above `N` | 20 |
| `--summary` | JSON: drop per-file, duplicate, and finding arrays while retaining explicitly requested context/graph/directory/impact blocks | off |
| `--context` | Add a deterministic, token-budgeted reading plan | off |
| `--no-context` | Disable a context plan enabled by configuration | off |
| `--context-budget <TOKENS>` | Hard aggregate token budget; also enables the context plan | `32000` |
| `--context-max-files <N>` | Maximum files in the plan; also enables it | `25` |
| `--focus <PATH>` | Prioritize a file/directory, nearby siblings, matching tests, and its supported graph neighborhood; repeatable and enables the plan | — |
| `--baseline-ready` | Emit compact JSON with the complete, versioned finding catalog | off |
| `--dup-mode <strict\|mild\|weak>` | Trivia filtering: keep all; ignore whitespace; or also ignore comments | `mild` |
| `--dup-format-scope <exact\|compatible\|all>` | Candidate pools; `compatible` combines JS/TS/TSX only | `exact` |
| `--health-scope <source\|all>` | Files eligible for marker and duplication health analysis | `source` |
| `--health-include <FORMAT>` | Add HTML, CSS, SCSS, JSON, YAML, TOML, Markdown, XML, or Text to the source health corpus; repeatable | — |
| `--dup-snippets` | Include bounded source snippets in full duplicate findings | off |
| `--dup-details` | Show precise pair findings in table/Markdown output | off |
| `--by-dir[=DEPTH]` | Add a per-directory rollup at path depth `DEPTH` (default 1) | off |
| `--since <REF>` | Restrict the scan to files changed since a git ref (commit/branch/tag) | — |
| `--staged` | Restrict the scan to staged (index) changes | off |
| `--working` | Restrict the scan to uncommitted working-tree changes | off |
| `--graph` | Build an import and explicit type-relationship graph for every first-class language | off |
| `--graph-focus <PATH>` | Restrict graph output to a file/directory neighborhood; repeatable and enables the graph | — |
| `--graph-depth <N>` | Maximum focus traversal hops (`0..64`; `0` selects only focus files) | 1 |
| `--graph-direction <dependencies\|dependents\|both>` | Edge direction followed from graph focus paths | `both` |
| `--impact` | With a diff scope, report direct/transitive internal dependents | off |
| `--review[=lines\|deep]` | With a diff scope, report findings intersecting changed lines; `deep` compares both Git snapshots | off |
| `--fail-on-review` | Exit code 2 for current line findings, or new/worsened deep findings | off |
| `--baseline <FILE>` | Compare against a previously saved JSON report and show deltas | — |
| `--fail-on-regression` | Exit code 2 if any metric regressed versus `--baseline` | off |
| `--fail-on <EXPR>` | CI gate; nonzero exit if met | — |
| `-q, --quiet` | Suppress the progress bar | off |

### Live daemon and web UI

The daemon serves the existing `ScanReport` contract and watches the target for source changes:

```sh
reposcout daemon .
```

It binds to `127.0.0.1:7331` by default. The web workspace uses pnpm and contains a React 19,
TypeScript 6, Vite 8, Vitest, Tailwind CSS 4, and Shadcn `new-york`/neutral dashboard, plus a
bespoke public landing page built on the same frontend stack without Shadcn:

```sh
pnpm install
pnpm dev:web
pnpm dev:landing
```

Vite proxies `/api` to the daemon, so run the daemon and web commands in separate terminals.
The dashboard defaults to the system color scheme and supports explicit light/dark selection.
Every view has a bookmarkable route (`/overview`, `/risk`, `/complexity`, `/duplication`, `/files`,
`/findings`, and `/graph`), participates in browser back/forward history, and survives direct
refreshes through Vite's SPA fallback. Graph routing continues below the tab: architecture scopes
use `/graph/scope/<repository-path>`, focused files use `/graph/file/<repository-path>`, and
non-default presentation, direction, and depth controls use canonical query parameters. Non-Graph
views use reusable Shadcn/TanStack data grids with search, sorting, column visibility, selectable
page sizes, and client-side pagination over the complete report data rather than a hard-truncated
first page.
Its Graph tab deliberately requests topology only when opened. The resulting revision-keyed graph
is cached by the daemon and begins with deterministic architecture groups. Redundant single-child
project/area/package chains are collapsed, and a selectable parent group already contains its
useful immediate child scopes and files instead of requiring several empty drill-down steps.
One click only selects a child and isolates its direct relationships. Double-clicking a file with
resolved type relationships opens a semantic type-structure neighborhood: the selected type is
large and central, explicit extenders/implementors/bases/contracts are separated into labeled
parent groups, and ordinary direct imports sit in quieter bounded context groups. Full neighborhood
restores the normal dependency, blast-radius, and depth controls; files without explicit type
relationships open that normal neighborhood directly. Double-clicks, scope changes, and breadcrumb
steps update browser history, while one-click inspection remains transient. Search covers every
scanned file, and factual scope, file, relationship-group, and connection inspectors expose scan
metrics and resolver provenance. Dense file views use a spaced left-to-right layout, subdued idle
edges, readable minimum zoom, and a high-contrast navigable minimap. Focused cross-directory
neighborhoods add path-based React Flow parent groups whose labels distinguish module, package,
namespace-path, and mixed-language scopes.
File size reflects resolved structural reach rather than file length: explicit inheritance,
implementation, trait, and embedding reach takes precedence, followed by measured import
dependents/coordinator breadth. Type relationships stay distinct from import edges, and ambiguous
names do not create speculative links. The semantic projection is language-agnostic over the
resolved symbol topology, so every first-class language and mixed repository uses the same
interaction. The browser renders at most 100 nodes including group containers, and ordinary
watched scans do not pay the graph-analysis cost.
The landing page is independent from the daemon and builds to `apps/landing/dist`.

Daemon options:

| Flag | Description | Default |
|------|-------------|---------|
| `--host <ADDRESS>` | HTTP bind address | `127.0.0.1` |
| `--port <PORT>` | HTTP port | `7331` |
| `--debounce-ms <MS>` | Filesystem event coalescing delay | `300` |
| `--profile <full\|lite>` | Analyzer set; `lite` omits whole-corpus duplication and Git churn | `full` |

HTTP endpoints:

| Endpoint | Purpose |
|----------|---------|
| `GET /api/health` | Process health and version |
| `GET /api/snapshot` | Daemon state plus the latest successful report |
| `GET /api/graph?revision=N` | Build or reuse the graph for a completed report revision |
| `GET /api/events` | `scan_started`, `scan_completed`, and `scan_failed` SSE events |
| `POST /api/rescan` | Queue a manual scan; repeated requests are coalesced |

The default `full` profile runs every analyzer. Use `reposcout daemon --profile lite .` to omit
duplication and Git churn when their whole-corpus runtime is unsuitable; the report's
`analysis_profile` records that those metrics were not run, and the dashboard labels them
accordingly.

Scans are single-flight and execute away from the async HTTP runtime. Filesystem bursts collapse
to one pending refresh, and clients can continue reading the last successful report while a new
scan runs. This matters on large repositories: the incremental caches eliminate repeated per-file
analysis and reuse immutable Git commit events, but a first-time whole-corpus duplication or
unbounded Git-history pass in the `full` profile may still take substantial time. The daemon
reports scan start time and state rather than imposing a timeout. Shutdown does not wait for a
long-running analyzer pass to complete.

Graph analysis is independently single-flight. The dashboard's overview ranks the architectural
core and never renders more than 100 nodes at once; file search can focus any supported file, and
one-to-three-hop controls project its dependencies, reverse dependents (blast radius), or both.
Node selection exposes exact direct neighbors and the resolver used for every visible import.

The service has no authentication. Keep the default loopback bind unless every client on the
chosen network may read source-derived repository metrics.

### Examples

```sh
# Human summary of the current repo
reposcout

# Agent-friendly JSON for a subdirectory
reposcout -f json src/ > status.json

# Compact scouting payload for an agent (a few KB, not megabytes)
reposcout -f json --summary --profile agent src/lib/storage/

# Discover the installed contract without touching a repository
reposcout capabilities -f json

# Find a declaration across a mixed-language repository
reposcout locate HttpClient . --exact --kind class -f json

# Scout an untrusted checkout with bounded work and no repository-owned config
reposcout -f json --summary --profile safe .

# Receive a machine-readable failure on stderr
reposcout --error-format json --graph-depth 65 .

# Try the ordinary first-run command while preserving live diagnostics
reposcout --debug-log /tmp/reposcout-debug-2026-07-17.jsonl .

# Explainable reading plan that cannot exceed 24k tokens or 15 files
reposcout -f json --summary --context-budget 24000 --context-max-files 15 .

# Center the plan on one file, nearby siblings, matching tests, and supported graph neighbors
reposcout -f json --summary --focus src/service.ts .

# Keep metrics change-scoped while planning the tests and blast radius worth reading
reposcout -f json --working --context --impact .

# Markdown summary written to a file (format inferred from extension)
reposcout -o STATUS.md src/

# Token count with the cl100k_base encoding
reposcout tokens --encoding cl100k_base src/

# Only duplication and complexity
reposcout --only dup,complexity src/

# ESLint-style per-function complexity findings with a stricter maximum
reposcout complexity --max-complexity 12 src/

# Ignore comment-only differences and show precise pair findings
reposcout dup --dup-mode weak --dup-details src/

# Include authored styles in health analysis, or deliberately scan every recognized format
reposcout --health-include css --health-include scss .
reposcout --health-scope all .

# Per-directory rollup two levels deep
reposcout --by-dir=2 src/

# Scout only what changed since main (great before a review or a sub-agent spawn)
reposcout -f json --summary --since main src/

# Full mixed-language import graph with stable adjacency/edges
reposcout --graph src/

# One service's two-hop blast radius, as Mermaid ready for Markdown/GitHub
reposcout --graph-focus src/service.ts --graph-direction dependents --graph-depth 2 \
  -f mermaid -o service-radius.mmd .

# A DOT dependency neighborhood; output extension enables graph analysis
reposcout --graph-focus src/service.ts --graph-direction dependencies -o service.dot .

# Blast radius of the current change set (full topology, scoped metrics)
reposcout -f json --working --impact .

# Fast changed-line review, or a two-snapshot review with resolved/improved states
reposcout --working --review .
reposcout -f sarif --since main --review=deep --fail-on-review .

# Save a compact finding-complete baseline, then gate later changes
reposcout --baseline-ready -o baseline.json src/
reposcout --baseline baseline.json --fail-on-regression src/

# Explain why one file matters in its full repository context
reposcout explain src/service.ts

# SARIF for code-scanning / CI, or NDJSON for streaming consumers
reposcout -o report.sarif src/
reposcout -f ndjson src/ > report.ndjson
```

### Debugging slow or crashing runs

Pass the global `--debug-log <FILE>` option to any command. The path must not already exist, so
each invocation keeps an independent diagnostic record instead of truncating an earlier log or an
accidentally selected source file. The path is resolved exactly and excluded from scan discovery;
daemon mode also ignores its watcher events.

```sh
reposcout --debug-log /tmp/reposcout-debug-2026-07-17.jsonl .
```

Each flushed line is one JSON object with `schema_version`, `timestamp`, `elapsed_ms`, `sequence`,
`thread`, `event`, and event-specific `data`. `stage_start` / `stage_end` isolate broad phases;
`discovery_progress` samples the walker every 1,000 selected files; `scan_stage` identifies
expensive work such as Git history, exact/Type-2 duplication, graph, or context analysis; and
paired `file_start` / `file_end` records expose the path, worker thread, status, and elapsed time.
An unmatched start record identifies work that was in flight when the process stopped. `render_*`
and `output_*` records distinguish analysis delays from report serialization or writing.

While the command is otherwise quiet, an independent `heartbeat` is flushed every two seconds.
It records the last non-heartbeat event, how long that event has been quiet, and, on Linux, current
and peak resident memory. This keeps the file visibly growing even during a long allocation or
sort that cannot expose an inner counter.

Type-2 detection additionally emits `type2_progress` records at every phase transition and at
most once per second during long loops. The phases cover per-format-pool indexing, candidate
planning/search, match sorting, overlap suppression, group materialization, and final sorting.
Records include totals and completed counts for files, windows, fingerprint/candidate buckets,
bounded seed pairs, verification-token comparisons, retained matches, and groups, plus elapsed
time and throughput. Candidate planning shows both total and admitted work. `pool_finished` and
`finished` state whether analysis is partial and whether the seed-pair, match-buffer, or overlap-
suppression bound was reached. A stopped counter identifies the exact operation; a growing counter
with falling throughput distinguishes slow progress from a deadlock.

Rust panics add a flushed `panic` record with the message, source location, and forced backtrace
before the normal panic handler runs. Abrupt termination such as `SIGKILL`, an OS-level crash, or
an allocator abort cannot add a final record, but all earlier flushed events remain available.
Debug logs contain paths, command arguments, configuration choices, and backtraces, but never file
contents; review them before sharing outside the environment.

### Scan modes

- **Agent context plan** (`--context`) ranks files already analyzed by the scan and emits a
  top-level `context` block with exact token accounting and selection reasons. The plan prefers
  focus paths, same-directory siblings, supported graph neighbors, matching tests, repository
  instructions, manifests, entry points, risky code, churn, and complexity. Selected Rust,
  Python, JavaScript, TypeScript/TSX, Go, and PHP files also expose compact declaration headers in
  `context.files[].symbols`; no source bodies are embedded. `outline_bytes`, omission counts, and
  `planning_ms` make that bounded structural payload observable. Outlines retain at most 16
  symbols (up to four non-exported declarations) and 2 KiB per file, with a 16 KiB plan-wide cap.
  Generated/minified/vendored files are skipped unless explicitly focused or changed. The ranker does not
  pack source or call a model and never exceeds `--context-budget` / `--context-max-files`;
  optional graph enrichment reuses one graph pass. Graph evidence covers every first-class
  language; other recognized languages still receive non-graph ranking signals.
- **Diff-scoped** (`--since <ref>` / `--staged` / `--working`) narrows the scan to files
  changed in that git diff, so an agent can assess just a changeset. Aggregates,
  duplication, and requested graph output are computed over the narrowed set. When context
  planning is also enabled, `context.changed_files` automatically seeds a separate full-tree
  planning universe so matching tests, direct dependencies, direct/transitive dependents, and
  nearby high-risk files can be selected without widening those metrics. Each
  `context.files[].evidence` record carries its role, hop distance, resolver, and `high` or
  `partial` confidence; deleted changed paths remain valid seeds. `context.planning_diagnostics`
  reports coverage of that separate universe instead of hiding unreadable or unsupported files
  behind the primary diff-scoped diagnostics.
- **Change impact** (`--impact`, with a diff scope) keeps those metrics scoped but builds a
  separate full-tree first-class-language topology. It reports changed graph files plus unchanged
  direct/transitive dependents, unresolved local imports, and a confidence level. When the
  target is a subpath, changed files remain scoped to that target while dependents may live
  anywhere in the repository. Confidence is conservative when graph files cannot be read or
  parsed cleanly; `parse_errors` reports the syntax error-node count.
- **Per-directory rollup** (`--by-dir[=DEPTH]`) adds a `directories[]` array (and a "By
  directory" table) with per-subtree tokens, SLOC, complexity, duplication, and source-file
  counts without a matching test — scout several subtrees in one invocation instead of N runs.
- **Baseline compare** (`--baseline <file>`) accepts full or compact `--summary` JSON and
  reports per-metric deltas. A `--baseline-ready` artifact additionally compares the complete
  finding catalog as `new`, `resolved`, `worsened`, and `improved`; new/worsened findings count
  as regressions. Analyzer, encoding, target, diff-scope, health-file policy, duplication
  settings, and finding settings must match. Diff-scoped profiles also compare the resolved base tree, so different
  names for the same tree are compatible but a moved ref is not. The baseline file itself is
  excluded when it lives under the target. Reports without `analysis_profile` metadata must be
  regenerated because their all-content health semantics are not compatible with the source-first
  default. Compatible reports without a finding catalog retain aggregate-only comparison. Add
  `--fail-on-regression` to exit `2` on a regression.
- **Changed-line review** (`--review`, with a diff scope) filters current complexity, marker,
  and duplicate findings to regions touched by the diff. `--review=deep` analyzes the Git base
  and current snapshot with the same configuration, preserves identities across detected
  renames, and reports four-way finding states. Staged review reads index content, including
  when the worktree differs. `--fail-on-review` exits `2` for fast-mode findings or deep-mode
  `new`/`worsened` findings; resolved/improved findings remain informational.
- **Dependency graph** (`--graph`) extracts imports and explicit type relationships from every
  first-class language's tree-sitter syntax tree and combines them into one topology for
  mixed-language repositories. It resolves
  relative JS/TS imports plus `tsconfig.json` / `jsconfig.json` `baseUrl` and `paths` mappings
  (following local project references and relative extends), local `package.json`
  `exports`, `imports`, package subpaths and entrypoints, and `.js`/`.jsx`/`.mjs`/`.cjs` imports whose
  checked-in source is TypeScript. Python resolution covers relative imports, unambiguous
  repository-absolute modules, and conventional `src/` roots. PHP resolution covers namespace
  imports through the nearest Composer `autoload` / `autoload-dev` PSR-4 and PSR-0 mappings,
  common `src` / `app` / `lib` layouts when no mapping applies, and statically evaluable
  `include` / `include_once` / `require` / `require_once` paths. Rust resolution covers external
  `mod` declarations, `#[path]` overrides, `crate` / `self` / `super` uses, and unambiguous local
  Cargo library names. Go resolution maps module-local imports from `go.mod` and relative imports
  to one stable, non-test-preferred representative file per package. Full machine
  output includes path-sorted `files[]` adjacency and `edge_list[]` records whose `resolver`
  identifies relative, Python-relative/absolute/src-root, Composer, PHP-include, Rust module/use/
  workspace, Go module/relative-package, configured package-metadata, or fallback alias resolution.
  The additive `symbols[]` and `symbol_edges[]` contract records resolved declarations and explicit
  `extends`, `implements`, and embedding relations separately from file imports. Resolution accepts
  qualified names, same-file/same-scope names, and globally unique short names; ambiguous short
  names are counted as unresolved rather than guessed. The web explorer includes those proven type
  edges in local neighborhoods. CLI focus queries traverse import dependencies, reverse import
  dependents, or both under a hard `--graph-depth` bound; unmatched
  focus paths, syntax errors, unresolved local imports, and invalid resolver configs stay visible.
  Package metadata only creates internal edges for deterministic local `./` targets under the
  supported source/import/default/node/require/types conditions; duplicate package names are
  treated as ambiguous config rather than resolved arbitrarily.
  DOT and Mermaid are pure text renderers over the same selected graph. Python sibling imports
  such as `from . import helpers` resolve to `helpers.py`. The graph remains heuristic: Rust use
  paths are module-level rather than symbol-level, and Go package edges point to a representative
  file so callers can see package topology without mistaking it for precise file references.

## Scouting a path (for agents)

`reposcout -f json --summary <path>` is designed to answer questions an agent asks
*before* diving into a directory or spawning a sub-agent:

- **"Will this path fit in a context window?"** → `summary.tokens` (and `summary.files`).
- **"Just tell me — is it worth cleaning, and does it fit?"** → `summary.assessment`, a
  one-glance verdict with `cleanup_worth`, `fits_context`, an estimated `token_budget`,
  and human-readable `reasons`.
- **"Is this worth cleaning up?"** → `summary.duplication.duplicated_pct`,
  `summary.top_duplicates` (the highest-impact clone blocks — ranked by removable
  lines, with `copies`, `similarity`, and `locations`),
  `summary.complexity.functions_over_threshold`, and `summary.complexity_violations`
  (functions/methods over the configured cyclomatic maximum, with `path:line`).
- **"Which files should I actually read — or skip?"** → `summary.skip_candidates`
  (generated/minified/vendored files not worth reading, with a reason), `summary.top_risks`
  (source files ranked by a stable weighted size/complexity/churn score, saturated at
  1,000 SLOC, cyclomatic 100, and 20 commits, then multiplied by 1.10 when no matching
  test file or inline Rust test is found), and
  `summary.symbols` (function/type/export counts).
- **"Where is this type or callable declared?"** → `reposcout locate <symbol> <path> -f json`.
  Results are deterministically ranked, include path/line/signature/language/kind/export status,
  and can be constrained with exact, kind, language, and result-limit options.
- **"Give me a bounded reading order for this task."** → add `--context` and optionally
  repeat `--focus <path>`. Top-level `context.files` stays within the requested token/file
  limits and explains every choice; `context.files[].symbols` provides bounded body-free
  declarations, and `context.omitted` explains representative exclusions. Combine `--context`
  with a diff scope to seed the plan from the change automatically without widening scan metrics.
- **"Is there an obvious matching test?"** → `summary.test_presence` (test vs source
  file split plus conventional filenames—including PHPUnit `SomethingTest.php`—and
  inline-Rust-test matching; this is not measured coverage).
- **"Where is the risk concentrated?"** → `summary.top_hotspots` (churn × complexity,
  code files only) and `summary.top_token_files`.
- **"Did the scan actually cover the path?"** → top-level `diagnostics`
  (discovered/analyzed/unsupported/unreadable files, walker errors, and any partial Type-2
  analysis with omitted-work counts).
- **"What can this installed binary do?"** → `reposcout capabilities -f json`, without a scan.
- **"What else could this change affect?"** → `--since`/ `--staged`/ `--working`
  together with `--impact`, for every first-class language.
- **"Why does this particular file matter?"** → `reposcout explain <file>` for its inclusion
  status, exact ignore rule when available, risk calculation, matching tests, dependencies,
  dependents, and canonical findings.

The assessment's duplication signal is deliberately narrower than the raw duplication
summary: it triggers above 15% using physical-line coverage from non-test code files only.
`summary.duplication` still covers the entire selected scan scope, including tests and recognized
non-code formats, for diagnostics and CI gates. If `diagnostics.type2_analysis_partial` is true,
its exact-clone contribution is complete while its Type-2 contribution remains a lower bound.

The `--summary` payload is a few KB (the full report, with every file and duplicate
group, can be megabytes), so it drops cleanly into an agent's context. Explicitly requested
context, graph, impact, and directory-query results remain available in summary mode:

```sh
reposcout -f json --summary src/lib/storage/
```

Assessment booleans are evidence-qualified. `fits_context_known` is false when token analysis did
not run; `cleanup_worth_complete` is false when complexity, duplication, or churn evidence was
disabled; and `unavailable_signals` names every missing input. A disabled analyzer therefore never
turns a synthetic zero into a confident recommendation.

For schema compatibility, JSON fields named `untested_source_files`, `untested_samples`,
and `untested` remain unchanged. They mean that RepoScout found no matching test filename
or inline Rust test; they do not claim code-coverage measurement.

Line/comment metrics for Rust, Python, JavaScript, TypeScript/TSX, Go, and PHP use tree-sitter
comment ranges, so comment delimiters inside strings remain code. Other formats use a
quote-aware fallback scanner. Full JSON marks those files with
`line_metrics_approximate: true`, and the summary reports their count in
`line_metrics_approximate_files`.

Complexity is reported **per function**, and only for real code — prose, data and
markup files (Markdown, JSON, YAML, HTML, CSS, …) are counted for tokens and lines but
never assigned cyclomatic/cognitive scores, so they don't pollute the complexity view
or the hotspot ranking. The default rule flags cyclomatic complexity above `20` (a score
of exactly `20` is allowed); change it with `--max-complexity`. This reporting threshold
does not change the analyzer or automatically change the exit code. For CI enforcement,
pair it with the equivalent `--fail-on`, for example
`--max-complexity 12 --fail-on 'max-cyclomatic>12'`.
Python comprehension clauses and JavaScript/TypeScript default values, logical assignments,
optional chains, and nullish coalescing count as control-flow paths. File-level cyclomatic
values aggregate independent function scopes, while summary averages and gates remain
strictly per-function. Cognitive complexity includes direct self-recursion.

Maintainability Index uses Microsoft's normalized `0..100` formula, with SLOC as the
cross-language source-operation proxy. The interpretation bands are `0..9` low, `10..19`
moderate, and `20..100` good; comments do not directly increase the score.
Halstead volume, difficulty, and effort use the published equations, but operator/operand
classification comes from each grammar's leaf tokens (or the generic fallback). Treat those
values as stable RepoScout signals, not as cross-language or cross-tool equivalents.

### Metric interpretation and limits

RepoScout's metrics are intended to be consistent repository-scouting signals, not exact
substitutes for language-specific analyzers such as ESLint, Sonar, or Visual Studio. Core
formulas, first-class-language line classification, common control-flow constructs, and report
aggregation are covered by conformance fixtures and end-to-end tests. The remaining limits are:

- line/comment metrics for formats without a bundled grammar use the explicitly marked
  approximate fallback;
- Maintainability Index uses Microsoft's normalized formula with file-level SLOC as a
  cross-language proxy, rather than Microsoft's language-specific logical-operation inputs;
- cognitive complexity counts direct self-recursion but not mutual recursion cycles or every
  language-specific Sonar nuance;
- JavaScript class-field initializers and static blocks are not modeled as separate implicit
  function scopes as they are by ESLint; and
- Halstead token classification, composite risk, test-file matching, and the overall assessment
  are RepoScout-specific heuristics that should be compared over time within RepoScout rather
  than directly against another tool's numbers.

Duplication matches repeated **structured token sequences**, not semantics. Exact
matching preserves identifier and literal values; Type-2 matching allows consistent
identifier renames and same-category literal changes. `top_duplicates` keeps the legacy
group ranking, while `top_duplicate_findings` adds stable pair IDs and precise locations.
By default the corpus is authored program/build source; explicit health includes or all scope
widen both duplication and marker analysis without removing those formats from ordinary inventory.
Percentages use physical-line and duplication-lexer-token unions over the eligible corpus, so
overlapping exact and near findings are not double-counted and excluded content cannot dilute the
denominator. A few entries may still be real-yet-not-
extractable (e.g. identical per-file import blocks). Vendored or generated
trees (a bundled UI component library, `dist/`, codegen output, …) can dominate the
ranking — exclude them with `excludes` in `reposcout.toml` or `--exclude <glob>`.
Type-2 `similarity` is a weighted structured-token score, not textual edit similarity:
exact tokens receive `1.0`, consistently renamed identifiers `0.80`, and changed literals
of the same category `0.70`.

Pathologically repetitive pools are admitted deterministically from the rarest fingerprint
buckets first. Per format pool, RepoScout examines at most 10,000,000 candidate seed pairs,
buffers at most 250,000 compact matches, and performs at most 10,000,000 overlap checks. Ordinary
repositories below those bounds retain the complete detector behavior. If a bound is reached,
exact-clone analysis remains complete but Type-2 findings and combined duplication percentages are
lower bounds. Table/Markdown output says the Type-2 result is partial; JSON exposes
`diagnostics.type2_analysis_partial` plus skipped-pair/match and limit-reason fields. The installed
values are also available from `reposcout capabilities -f json`.
There is no CLI override for these bounds. `ROADMAP.md` records a higher-effort mode as an
evidence-gated possibility rather than a committed follow-up.

## CI gates (`--fail-on`)

Provide one or more comma-separated `key OP number` conditions. If **any** condition
is true, `reposcout` exits with code **2** (regular errors exit 1).

```sh
reposcout --fail-on "max-cyclomatic>30,duplicated-pct>5,min-mi<50" src/
```

Supported keys: `max-cyclomatic`, `avg-cyclomatic`, `max-cognitive`, `avg-cognitive`,
`min-mi` (a.k.a. `min-maintainability`), `avg-mi` (`avg-maintainability`),
`duplicated-pct`, `tokens`, `files`, `sloc`. Operators: `>`, `<`, `>=`, `<=`, `==`.
Conditions that require a disabled analyzer are rejected instead of evaluating a synthetic zero.

The cyclomatic and cognitive gates are **per function** — `max-cyclomatic>30` fires when
any single function exceeds 30, and `avg-cyclomatic` is the mean across all functions.

For change-over-time gating, save a JSON report as a baseline and compare later with
`--baseline <file> --fail-on-regression`: reposcout exits `2` if any tracked metric
(duplication, maintainability, maximum complexity, or untested-source count) got worse,
or if a compatible finding catalog contains a new/worsened finding, printing the specific
regressions. Token and SLOC deltas remain informational. Prefer `--baseline-ready` when the
artifact exists only for comparison: it omits heavy file/duplicate arrays while retaining every
finding rather than a top-N projection.

For review gating, combine one diff scope with `--review --fail-on-review`. Fast review gates on
any current finding touching changed lines. Deep review gates only on `new` and `worsened`, so
`resolved` and `improved` findings can be reported without failing CI.

## Configuration

RepoScout resolves settings in this order, from weakest to strongest:

1. built-in defaults;
2. the global config in the OS config directory;
3. the nearest project `reposcout.toml` (or `.reposcout.toml`) found from the target upward;
4. command-line flags.

The global path follows the operating system: for example,
`~/Library/Application Support/reposcout/reposcout.toml` on macOS and
`${XDG_CONFIG_HOME:-~/.config}/reposcout/reposcout.toml` on Linux. Set
`REPOSCOUT_GLOBAL_CONFIG` to an explicit file path for hermetic automation. Missing files are
normal. Invalid/unknown keys fail with the source path and TOML setting instead of silently
falling back.

Only explicitly defined fields override a lower layer. Nested context fields merge independently,
while list fields such as `markers`, `health_includes`, and `excludes` replace the lower-layer
list; repeated CLI `--exclude` and `--health-include` values extend their effective config lists.
This lets a team commit project policy while
each developer keeps unrelated personal defaults.

Inspect the complete resolution before a scan (JSON is convenient for agents):

```sh
reposcout config .
reposcout config -f json path/to/subdirectory
```

The output reports precedence, both candidate source paths, whether each was loaded, which keys it
defined, and every effective file-configurable value. All project/global keys are optional:

For automation, `--no-project-config` leaves global/user defaults in place but does not parse or
apply the repository file; config inspection marks that discovered source as ignored. The built-in
`--profile agent` disables whole-corpus duplication and Git churn unless an analyzer subcommand or
`--only` explicitly requests them. `--profile safe` implies the same cheap defaults, ignores the
project file, caps workers/top lists/history/context, requires normal ignore handling, excludes
hidden files and lockfiles, forces the source-only health corpus, and constrains any explicitly
requested duplication run. Effective
settings and `execution.safety_limits` make every override visible.

```toml
encoding = "o200k_base"          # or "cl100k_base"
jobs = 8                          # worker threads
use_cache = true
top = 10                          # length of top-N lists
max_complexity = 20               # maximum cyclomatic complexity per function/method
include_hidden = false
respect_gitignore = true
exclude_lockfiles = true          # skip Cargo.lock, package-lock.json, go.sum, …
excludes = ["vendor/**", "*.min.js"]
markers = ["TODO", "FIXME", "HACK", "XXX", "BUG"]
health_scope = "source"             # source (default) or every recognized format
health_includes = []                 # html, css, scss, json, yaml, toml, markdown, xml, text
min_dup_tokens = 50               # minimum token run for a clone
min_dup_lines = 3                 # drop clones spanning fewer lines (kills 1-line noise)
near_dup_min_similarity = 0.85    # [0,1] threshold for near-duplicates
duplication_mode = "mild"          # strict, mild, weak (weak ignores comments)
duplication_format_scope = "exact" # exact, compatible (JS/TS/TSX), all
duplication_report_snippets = false
churn_max_commits = 5000          # cap history walked for churn (0 = unlimited)

[context]
enabled = false                    # opt in for every full scan in this project
budget = 32000                     # hard aggregate selected-token budget
max_files = 25                     # hard selected-file cap
```

## Output & JSON schema

The JSON output is stable and versioned via `schema_version`. Top-level shape:

```jsonc
{
  "schema_version": "1.0",
  "root": "/abs/repo",
  "target": "src/",
  "generated_at": "2024-01-01T00:00:00+00:00",
  "encoding": "o200k_base",
  "analysis_profile": {
    "analyzers": { "tokens": true, "complexity": true, "imports": true, "markers": true, "duplication": true, "churn": true },
    "diff_scope": "full",
    "duplication": { "min_tokens": 50, "min_lines": 3, "min_similarity": 0.85, "mode": "mild", "format_scope": "exact" },
    "health": { "scope": "source", "includes": [] },
    "findings": { "catalog_version": 1, "max_complexity": 20, "markers": ["TODO", "FIXME"], "risk_algorithm_version": 3, "risk_threshold": 0.7 }
  },
  "execution": {
    "profile": "agent",
    "config_mode": "project",
    "project_config": "/abs/repo/reposcout.toml",
    "safety_limits": [],
    "stage_ms": { "discovery": 1, "file_analysis": 12, "cross_file": 0, "planning_universe": 0, "report_assembly": 2, "total": 15 },
    "cache_enabled": true,
    "cache_hits": 42,
    "cache_misses": 3,
    "cache_enrichments": 2,
    "graph_fact_files": 40
  },
  "diagnostics": { "discovered_files": 0, "analyzed_files": 0, "unsupported_files": 0, "unreadable_files": 0, "walker_errors": 0 },
  "finding_catalog": {
    "version": 1,
    "findings": [ { "fingerprint": "…", "identity": "…", "kind": "complexity", "severity": "warning", "message": "…", "primary_location": { "path": "src/lib.rs", "start_line": 1, "end_line": 10 }, "related_locations": [], "metrics": { "cyclomatic": 24.0 } } ]
  },
  "summary": {
    "files": 0, "bytes": 0, "tokens": 0,
    "loc": 0, "sloc": 0, "comment_lines": 0, "comment_ratio": 0.0, "line_metrics_approximate_files": 0,
    "source": { "files": 0, "bytes": 0, "tokens": 0, "loc": 0, "sloc": 0, "comment_lines": 0 },
    "languages": [ { "name": "Rust", "source": true, "files": 0, "sloc": 0, "tokens": 0, "…": 0 } ],
    "complexity": { "cyclomatic_total": 0, "cyclomatic_avg": 0.0, "cyclomatic_max": 0, "cognitive_total": 0, "cognitive_avg": 0.0, "cognitive_max": 0, "mi_avg": 0.0, "mi_min": 0.0, "functions": 0, "cyclomatic_threshold": 20, "functions_over_threshold": 0, "approximate_files": 0 },
    "duplication": { "exact_groups": 0, "near_groups": 0, "duplicated_lines": 0, "duplicated_pct": 0.0, "analyzed_lines": 0, "duplicated_tokens": 0, "analyzed_tokens": 0, "duplicated_tokens_pct": 0.0, "by_language": [] },
    "markers": { "TODO": 0 },
    "top_token_files": [ { "path": "…", "tokens": 0 } ],
    "top_source_token_files": [ { "path": "…", "tokens": 0 } ],
    "top_hotspots": [ { "path": "…", "commits": 0, "cyclomatic": 0, "score": 0.0 } ],
    "top_functions": [ { "path": "…", "name": "…", "line": 0, "cyclomatic": 0, "cognitive": 0, "max_nesting": 0 } ],
    "complexity_violations": [ { "path": "…", "name": "…", "line": 0, "cyclomatic": 0, "cognitive": 0, "max_nesting": 0 } ],
    "top_duplicates": [ { "lines": 0, "tokens": 0, "similarity": 1.0, "copies": 0, "duplicated_lines": 0, "locations": ["path:start-end"] } ],
    "top_duplicate_findings": [ { "id": "…", "kind": "exact", "format": "Rust", "tokens": 0, "lines": 0, "similarity": 1.0, "removable_lines": 0, "locations": ["path:line:column-line:column"] } ],
    "symbols": { "functions": 0, "types": 0, "exports": 0 },
    "skip_candidates": [ { "path": "…", "reason": "minified", "tokens": 0 } ],
    "test_presence": { "test_files": 0, "source_files": 0, "untested_source_files": 0, "untested_samples": ["…"] },
    "top_risks": [ { "path": "…", "score": 0.0, "sloc": 0, "cyclomatic": 0, "churn_commits": 0, "untested": false, "reasons": ["…"] } ],
    "assessment": { "fits_context_known": true, "fits_context": true, "token_budget": 200000, "cleanup_worth": "low", "cleanup_worth_complete": false, "unavailable_signals": ["duplication", "churn"], "reasons": ["…"] }
  },
  "files": [
    {
      "path": "…", "language": "Rust", "bytes": 0,
      "tokens": 0, "loc": 0, "sloc": 0, "comment_lines": 0, "comment_ratio": 0.0,
      "line_metrics_approximate": true,  // present only for generic fallback line metrics
      "complexity": { "cyclomatic": 0, "cognitive": 0, "max_nesting": 0, "halstead": { }, "maintainability_index": 0.0, "functions": [ { "name": "…", "line": 0, "end_line": 0, "symbol_key": "…", "cyclomatic": 0, "cognitive": 0, "max_nesting": 0 } ] },  // omitted for non-code files (Markdown, JSON, …)
      "imports": ["std"],  // root dependencies only; relative/local imports are omitted
      "markers": { "TODO": 0 },
      "marker_occurrences": [ { "marker": "TODO", "line": 1, "column": 4, "context_hash": "…", "occurrence": 1 } ],
      "churn": { "commits": 0, "authors": 0, "first_commit": "…", "last_commit": "…" },
      "symbols": { "functions": 0, "types": 0, "exports": 0 },  // first-class languages only
      "skip_hint": "generated",                                  // present only when the file looks unreadable
      "has_inline_tests": false,
      "approximate": false
    }
  ],
  "duplicates": {
    "exact": [ { "lines": 0, "tokens": 0, "similarity": 1.0, "format": "Rust", "fingerprint": "dup:v1:…", "instances": [ { "path": "…", "start_line": 0, "end_line": 0, "start_column": 0, "end_column": 0, "start_byte": 0, "end_byte": 0, "start_token": 0, "end_token": 0 } ] } ],
    "near":  [ … ],
    "findings": [ { "id": "…", "family_id": "…", "kind": "type2", "format": "Rust", "tokens": 0, "lines_a": 0, "lines_b": 0, "similarity": 0.9, "confidence": "high", "normalization": "mild", "fragment_a": { "path": "…", "start_line": 0, "end_line": 0, "start_column": 0, "end_column": 0, "start_byte": 0, "end_byte": 0, "start_token": 0, "end_token": 0 }, "fragment_b": { "…": "…" }, "removable_lines": 0 } ],
    "file_coverage": [ { "path": "…", "format": "Rust", "lines": 0, "tokens": 0, "duplicated_lines": 0, "duplicated_tokens": 0, "duplicated_lines_pct": 0.0, "duplicated_tokens_pct": 0.0 } ]
  },
  // The blocks below appear only when their corresponding mode is requested:
  "context": {
    "strategy_version": 2,
    "planning_ms": 0,
    "budget_tokens": 32000,
    "selected_tokens": 0,
    "candidate_files": 0,
    "omitted_files": 0,
    "skipped_files": 0,
    "focus": ["src/service.ts"],
    "unmatched_focus": [],
    "change_scope": "working",
    "changed_files": ["src/service.ts"],
    "graph_languages": ["TypeScript"],
    "graph_unresolved_imports": 0,
    "graph_parse_errors": 0,
    "graph_config_errors": 0,
    "outline_symbols": 1,
    "outline_bytes": 150,
    "outline_omitted_symbols": 0,
    "planning_diagnostics": { "discovered_files": 10, "analyzed_files": 9, "unsupported_files": 1, "unreadable_files": 0, "walker_errors": 0 },
    "files": [ { "path": "src/service.ts", "tokens": 0, "score": 0.0, "reasons": ["changed in working scope"], "evidence": [ { "role": "changed", "confidence": "high", "distance": 0 } ], "symbols": [ { "name": "Service", "kind": "class", "signature": "export class Service …", "line": 1, "exported": true, "reasons": ["exported/public declaration"] } ] } ],
    "outline_only": [ { "path": "src/oversized.ts", "source_tokens": 50000, "score": 0.0, "reason": "file exceeds total token budget", "reasons": ["explicit focus"], "symbols": [ { "name": "LargeService", "kind": "class", "signature": "export class LargeService …", "line": 1, "exported": true } ] } ],
    "omitted": [ { "path": "src/large.ts", "tokens": 0, "reason": "does not fit remaining token budget" } ]
  },  // --context / --focus / context budget flags
  "directories": [ { "path": "src/", "files": 0, "tokens": 0, "loc": 0, "sloc": 0, "cyclomatic_avg": 0.0, "cyclomatic_max": 0, "mi_avg": 0.0, "duplicated_lines": 0, "untested_source_files": 0 } ],  // --by-dir
  "baseline": { "baseline_generated_at": "…", "metrics": [ { "metric": "tokens", "baseline": 0.0, "current": 0.0, "delta": 0.0 } ], "regressions": ["…"], "regressed": false, "finding_changes": { "comparison": "complete", "counts": { "new": 0, "resolved": 0, "worsened": 0, "improved": 0 }, "changes": [] } },  // --baseline
  "graph": { "languages": ["TypeScript"], "nodes": 2, "edges": 1, "files": [ { "path": "src/a.ts", "language": "TypeScript", "fan_in": 0, "fan_out": 1, "dependencies": ["src/b.ts"] } ], "edge_list": [ { "source": "src/a.ts", "target": "src/b.ts", "resolver": "tsconfig-paths" } ], "focus": ["src/a.ts"], "direction": "dependencies", "depth": 1, "cycles": [], "orphans": [], "top_depended": [], "most_dependent": [], "unresolved_imports": 0, "parse_errors": 0, "config_errors": 0, "config_files": ["tsconfig.json"] },  // --graph / --graph-focus
  "impact": { "changed_files": ["src/a.ts"], "graph_changed_files": ["src/a.ts"], "direct_dependents": ["src/b.ts"], "transitive_dependents": ["src/c.ts"], "unresolved_imports": 0, "parse_errors": 0, "config_errors": 0, "confidence": "high" },  // --impact + diff scope
  "review": { "mode": "deep", "scope": "working", "changed_files": [], "counts": { "current": 0, "new": 0, "resolved": 0, "worsened": 0, "improved": 0 }, "findings": [], "diagnostics": { "binary_files": 0, "unreadable_files": 0 } }  // --review + diff scope
}
```

`capabilities -f json` and `locate -f json` use smaller task-specific contracts rather than
embedding a `ScanReport`. A symbol query reports its normalized filters, total/returned counts,
truncation, first-class file coverage, execution telemetry, and `matches[]` with
`path`, `line`, `language`, `kind`, `name`, `signature`, `exported`, and stable relevance `rank`.
`locate -f ndjson` emits one query header followed by one record per match.

### SARIF & NDJSON

- **SARIF** (`-f sarif`, or `-o report.sarif`) emits a SARIF 2.1.0 document. Findings are
  surfaced as `results`: precise duplicate pairs with related locations
  (`reposcout/duplicate-code`), functions that exceed `--max-complexity`
  (`reposcout/high-complexity-function`), and — when `--graph` is on — orphan
  files (`reposcout/orphan-file`), each with `physicalLocation` regions. Drop it into GitHub
  code scanning or any SARIF-aware CI. With `--review`, results are filtered to the review
  set; deep states use SARIF `baselineState` (`new`, `updated`, or `absent`).
- **NDJSON** (`-f ndjson`, or `-o report.ndjson` / `.jsonl`) emits newline-delimited JSON:
  the first line is the aggregate `summary` (`"kind":"summary"`), followed by one line per
  file (`"kind":"file"`) and one per duplicate pair (`"kind":"finding"`). The summary
  record also carries schema/root/target/time/encoding/profile metadata, `diagnostics`, and,
  when requested, `context`, the full graph, `impact`, `baseline`, and review metadata. Duplicate records expose their
  exact/type2 category as `finding_kind`; review findings use `"kind":"review_finding"`.
  `--summary` keeps just the first line. Ideal for streaming into log
  pipelines or processing file-by-file without loading the whole report.

`-f dot` / `-o graph.dot` and `-f mermaid` / `-o graph.mmd` render the same deterministic graph
projection without invoking Graphviz or any other external process. `reposcout explain FILE`
supports table, JSON, Markdown, and single-record NDJSON output; graph-only and SARIF formats are
rejected because explain output is contextual evidence rather than a standalone graph or finding
set.

## Caching

Per-file results and Git-history events are cached in your OS cache directory (e.g.
`~/Library/Caches/reposcout/` on macOS, `~/.cache/reposcout/` on Linux). Because the cache lives
outside the repository, **scanning never writes anything into the scanned repo** — important when
an agent scouts many repositories. Per-file entries are keyed by the canonical scan root, content
hash, analyzer version, and encoding; they are invalidated automatically when a file, the tool
version, or the effective analysis profile changes (enabled analyzers, token encoding, and
markers plus their health-file eligibility). Subpath and diff scans merge their refreshed entries
into the root cache; only a complete
root scan prunes unseen files.
Declaration outlines and graph source facts share the per-file cache without changing its analysis
profile. Outlines enrich lazily for context/symbol lookup; import specifiers, parse diagnostics,
and explicit type-relation declarations/references enrich lazily for graph/context/impact/explain.
Later requests reuse both, while ordinary scans and daemon watch refreshes still avoid those
query-only AST traversals. `execution.cache_enrichments` distinguishes content-identical cache
hits that still paid this one-time query-fact extraction cost from fully warm hits.
Churn keeps a separate versioned index of immutable per-commit path changes and a small set of
exact `HEAD`/scope result views. An unchanged history therefore needs no Git traversal, while a
new or rewritten history reuses existing events and computes unseen commits. Cold and unseen
ordinary commits are sent through one NUL-delimited native `git diff-tree` stream instead of
constructing a libgit2 diff per commit. The root commit and only those delete/add pairs that can
rename a tracked path use libgit2; if the `git` executable is unavailable or rejects the stream,
RepoScout transparently uses libgit2 for the entire pass. Rename similarity is resolved lazily,
including when a different scan scope makes an older cached addition newly relevant.
Disable caching for one invocation with `--no-cache`. Use `reposcout cache clear [PATH]` to
remove both cache families for one canonical scan root; a repository subpath resolves to its Git
root, just like a scan. `reposcout cache clear --all` explicitly removes RepoScout's entire
OS-managed cache directory. Both forms are idempotent and report which locations were removed.
Avoid clearing a cache while a scan or daemon for the same repository is actively writing it,
because that process may recreate the cache immediately.

## Development

```sh
cargo build            # debug build
cargo test             # unit + integration tests
cargo run -- -f json . # run against this repo
```

Integration tests live in `tests/` and run the compiled binary against the fixture
tree in `tests/fixtures/`. `tests/fixtures/dup_languages.toml` is the canonical
31-format duplication corpus: each format has multi-line exact and Type-2 examples,
tested both through the frozen detector APIs and an end-to-end `reposcout dup` scan.
