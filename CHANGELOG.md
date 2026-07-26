# Changelog

All notable user-facing changes to `reposcout` are recorded here. Unreleased work stays
at the top, released versions are listed newest first, and new entries are prepended
within their section.

## [Unreleased]

## [0.1.0] - 2026-07-26

### Fixed

- Prevented pathological Type-2 duplication runs from turning repetitive format pools into
  multi-day, multi-gigabyte candidate searches. Covered diagonal ranges now use merged interval
  lookup instead of scanning every earlier region; candidate buckets are scheduled rare-first
  under deterministic per-pool seed-pair and compact-match bounds; and overlap suppression has
  its own comparison bound. Ordinary repositories below the limits retain complete results. A
  bounded run is never presented as complete: JSON/human reports and live debug events identify
  partial Type-2 analysis, the limit reached, and the omitted seed pairs or matches. Capability
  discovery advertises the installed bounds.

### Added

- Added a GitHub Releases distribution pipeline for checksum-verified macOS and Linux archives, a
  stable landing-page install URL, and `reposcout update` for installer-managed copies. The
  built-in updater follows only the latest stable GitHub release, preserves the recorded install
  location, and refuses source builds or stale/mismatched installer receipts.

- The web dashboard's Files table and the CLI's human file rankings now show file-total and
  per-callable average cyclomatic complexity side by side. Files without callable-level data
  distinguish the unavailable average from a measured zero; dashboard columns remain sortable.

- Made the zero-configuration `reposcout .` health view source-first. Complete repository
  inventory, token/context size, and line facts still include every recognized format, while
  marker and duplication analysis now defaults to programming/build source and excludes HTML,
  CSS/SCSS, JSON, YAML, TOML, Markdown, XML, and text noise. Repeatable
  `--health-include <FORMAT>` / `health_includes` opt in selected formats;
  `--health-scope all` / `health_scope = "all"` restores an explicit all-content corpus.
  Duplication line percentages now use eligible analyzed lines, profiles and caches record the
  policy, capabilities advertise it, and human reports collapse non-source inventory while
  ranking source files. Baselines without compatible health-policy metadata must be regenerated.

- Added opt-in `--debug-log <FILE>` diagnostics for slow, stalled, or crashing runs. The global
  flag writes schema-versioned NDJSON with the invocation, effective runtime configuration,
  discovery totals, top-level and detailed scan stages, per-file start/end timings across worker
  threads, rendering/output timings, runtime errors, and Rust panic locations/backtraces. A
  two-second heartbeat keeps quiet logs growing with last-event and resident-memory context;
  Type-2 duplication reports rate-limited indexing, candidate, verification, sorting, suppression,
  and materialization counters with throughput. Every record is flushed immediately; the fresh log
  path is excluded exactly from scans and daemon watch feedback, and RepoScout refuses to overwrite
  an existing file.

- Added `reposcout cache clear [PATH]` as an idempotent recovery/debugging escape hatch. It resolves
  repository subpaths to the canonical scan root and clears both per-file analysis facts and the
  separate Git-history cache. The explicit `--all` scope removes every OS-managed RepoScout cache,
  and capability discovery advertises the maintenance command.

- Added an agent-efficient CLI contract. `--profile agent` skips whole-corpus duplication and
  churn by default; `--profile safe` also ignores repository configuration and enforces explicit
  worker/history/context/duplication and discovery-policy guardrails; and `--no-project-config` exposes that trust
  boundary independently. `reposcout capabilities -f json` advertises the installed command,
  format, profile, bound, and language surface without scanning. `reposcout locate SYMBOL [PATH]`
  performs bounded case-insensitive ranked or case-sensitive exact declaration lookup across every
  first-class language with kind/language filters and JSON/NDJSON output. `--error-format json`
  makes both usage and runtime failures machine-readable. Scan and query reports now expose
  profile/config provenance, cache hit/miss/lazy-enrichment telemetry, reusable graph-fact
  coverage, and coarse stage timings.

- The public landing page now adds reduced-motion-aware parallax depth without shifting content
  through neighboring sections: the hero retains vertical dimensional movement, in-flow elements
  in selected showcases use bounded horizontal drift, signal-card visuals stay inside their cards,
  and the final orbit grows with scroll progress. Native CSS view timelines drive direct transform
  layers where supported; an animation-frame-coalesced passive scroll fallback covers older
  browsers without changing layout.

- The public landing page now introduces the mixed-language architecture graph as a first-class
  product surface. An interactive PHP, TypeScript, and Go neighborhood previews native package
  scopes, prominent type reach, direct relationship inspection, and the same bounded graph query
  through CLI JSON, DOT, or Mermaid output. The language overview now includes first-class PHP
  and the expanded graph formats.

- The web graph workspace is easier to scan, understand, and navigate. Hovering a node now
  traces its resolved relationships without changing the selection; zooming out swaps node
  cards to large glanceable name labels so wide views read like a map; and an in-canvas legend
  explains the visible languages, edge semantics, boundary styling, and size tiers for the
  current view. Scrolling pans the canvas (pinch or Ctrl/Cmd+scroll zooms), the expanded
  workspace gets a larger minimap, and nodes signal clickability on hover. The top metric strip
  adds hub-file and unresolved-import/parse-error counts alongside sharper cycle and orphan
  details, the details sidebar is wider, scope inspection gains line/comment, test-file, churn,
  and marker rollups, and file inspection gains line/comment and nesting facts.

- The web graph now routes its own navigation state instead of stopping at the top-level `/graph`
  tab. Architecture scopes use `/graph/scope/...`; focused files use `/graph/file/...`; and
  non-default type/full presentation, dependency direction, and hop depth are encoded as canonical
  query parameters. Direct refreshes and bookmarks restore the same graph level and controls,
  double-click navigation and breadcrumbs participate in browser history, and stale or malformed
  graph locations recover safely. Single-click highlights remain transient and do not flood
  history with inspection-only state.

- Double-clicking a file with resolved type relationships now opens a semantic type-structure
  neighborhood instead of mixing inheritance into a noisy multi-hop import graph. The selected
  type stays large and central; explicit extenders, implementors, embedded types, bases, and
  contracts occupy labeled React Flow parent groups on the appropriate side; and direct import
  context remains available in quieter, bounded groups with honest visible/total counts. This
  projection consumes the same syntax-proven symbol topology for every first-class language and
  mixed-language repository. A Full neighborhood action restores direction/depth traversal at any
  time, while files without explicit type relationships retain the normal neighborhood behavior.

- The web graph's architecture view now collapses redundant single-child project/area/package
  chains into selectable React Flow parent groups and shows their useful child scopes and files
  immediately. Groups participate in selection without becoming extra drill-down screens, while
  double-click still opens a child neighborhood. Explicit `extends`, `implements`, and embedding
  relationships are now collected conservatively across PHP, JavaScript/TypeScript, Python, Rust,
  and Go, kept separate from import edges, and surfaced in web traversal and the relationship
  inspector. Base classes, contracts, traits, and interfaces with broad direct reach render at a
  visibly larger measured size; ambiguous type names remain unresolved instead of creating
  invented links. Focused neighborhoods retain path-based groups, stable cross-group layout, and
  the browser's 100-node bound including containers.

- Added bookmarkable web dashboard routes for Overview, Risk, Complexity, Duplication, Files,
  Findings, and Graph. Tab changes now participate in browser history, direct routes restore the
  selected metric on refresh, page titles identify the active view, unknown routes recover to the
  overview, and Graph remains lazy/on-demand when opened directly at `/graph`.

- Replaced every non-Graph dashboard table with reusable Shadcn/TanStack data grids. Language,
  risk, complexity, duplication, file, and finding views now provide global search, sortable
  headers, column visibility, page-size controls, and bounded client-side pagination. Files and
  findings no longer discard everything after the first 100 records; their complete report arrays
  remain queryable while only the current page is rendered.

- Dependency graphs now cover every first-class language, including mixed-language repositories.
  Rust topology resolves external `mod` declarations, `#[path]` modules, `crate` / `self` /
  `super` uses, and local Cargo library names. Go topology resolves module-local package imports
  from `go.mod` plus relative package imports to a deterministic package representative. New
  `rust-*` and `go-*` edge provenance, contributing `Cargo.toml` / `go.mod` files, config
  diagnostics, context neighborhoods, explain output, impact analysis, and DOT/Mermaid exports all
  share the same bounded topology.

- PHP is now a first-class tree-sitter language across scanning and topology. PHP files receive
  syntax-aware line/marker metrics, per-function and closure complexity, symbol/public-API
  outlines, namespace-root imports, structured exact/Type-2 duplication tokens, common extension
  detection, and PHPUnit `*Test.php` matching. Graph, context, explain, impact, DOT/Mermaid, and the
  web explorer now resolve PHP namespace imports through Composer `autoload` / `autoload-dev`
  PSR-4 and PSR-0 maps, conventional source roots, and static include/require paths with resolver
  provenance and config diagnostics.

- Context plans now include compact, body-free symbol outlines for selected Rust, Python,
  JavaScript, TypeScript/TSX, Go, and PHP files. Export/public and representative declarations carry
  signatures plus selection reasons; per-file and total payload bounds, omission counts, serialized
  outline bytes, and incremental planning time keep the structural map measurable and controlled.

- Diff-scoped context plans are now change-aware. With `--since`, `--staged`, or `--working`,
  changed paths automatically seed a separate full-tree planning universe while ordinary report
  metrics remain diff-scoped. Plans rank changed files, direct dependencies, matching tests,
  direct/transitive dependents, and nearby risk under the existing hard budgets; structured
  evidence distinguishes precise direct syntax/config confidence from heuristic/transitive confidence.
  Deleted paths remain usable as virtual seeds, full-tree planning coverage has its own
  diagnostics, and the topology is shared with `--impact` while coexisting with `--review`.

- Graph resolution now understands deterministic local `package.json` `exports`, `imports`,
  package entrypoints and subpaths, including JavaScript-runtime-extension substitution to
  TypeScript sources. Python graphs now resolve unambiguous repository-absolute imports and
  conventional `src/` roots. Every new edge retains resolver provenance, ambiguous workspace
  package names remain unresolved with diagnostics, and zero-index heuristic scanning remains the
  default.

- Added an on-demand repository graph explorer to the live web dashboard. Opening the Graph tab
  requests a revision-keyed daemon analysis without adding graph work to normal watched scans,
  then provides search, dependency/blast-radius/bidirectional neighborhoods, hop controls,
  resolver provenance, node details, cycles, orphans, diagnostics, pan/zoom, and a minimap. The
  browser renders at most 100 graph nodes at once and the daemon reuses the graph until the scan
  revision changes.

- Added a queryable repository graph for agents and humans. Full JSON and NDJSON now expose
  deterministic per-file adjacency plus a stable internal edge list with resolver provenance.
  Repeatable `--graph-focus`, `--graph-depth`, and `--graph-direction` options return bounded
  dependency, dependent, or bidirectional neighborhoods, including unmatched-focus diagnostics.
  `-f dot` / `.dot` and `-f mermaid` / `.mmd` export the selected graph without requiring an
  external renderer.

- Added `tsconfig.json` / `jsconfig.json` `baseUrl` and `paths` resolution for JavaScript and
  TypeScript graphs, including JSON-with-comments, trailing commas, local project `references`,
  and relative `extends` files with cycle protection. Reports identify the resolver used by each
  edge, list contributing config files, count invalid resolver configs, and lower change-impact
  confidence when graph configuration cannot be trusted.

- Added an opt-in, deterministic agent context plan (`--context`, `--context-budget`,
  `--context-max-files`, and repeatable `--focus`) that converts existing scan facts into a
  hard-budgeted reading list. It prioritizes focus paths, supported direct dependencies and
  dependents, same-directory siblings, matching tests, repository instructions, entry points,
  risk, churn, and complexity;
  JSON/NDJSON and human reports include exact token usage, selection reasons, graph diagnostics,
  and bounded omission explanations. Summary JSON deliberately retains a requested plan.

- Added layered configuration with precedence `CLI > nearest project > global > defaults`.
  Project files override only explicitly defined global fields, nested context settings merge
  independently, and committed team configuration can coexist with personal defaults in the OS
  config directory. `reposcout config [PATH]` reports both source paths, loaded keys, precedence,
  and effective settings in table or JSON form; `REPOSCOUT_GLOBAL_CONFIG` supports hermetic
  automation.

- Added a bespoke, responsive RepoScout landing page on the existing React 19 / TypeScript 6 /
  Vite 8 / Tailwind CSS 4 stack. The page uses the supplied transparent artwork as its hero,
  explains the agent-first scouting workflow with representative structured output, and presents
  context fit, risk, duplication, change impact, install, and open-source entry points without
  introducing Shadcn or a second component system.

- Added the supplied RepoScout brand image to the web dashboard header and project README,
  retaining the high-resolution source asset shared by the landing-page hero.

- Added a dedicated web dashboard Duplication tab with exact/Type-2 totals, block size,
  similarity, copy counts, removable lines, locations, and explicit empty/not-run states.

- Added `reposcout daemon`, a localhost HTTP/SSE service with default all-analyzer `full` and opt-in
  fast `lite` analyzer profiles that watches a scan target, coalesces filesystem changes, preserves
  the last successful report during long scans, and supports manual rescans. Added a pnpm frontend
  workspace with a public landing package and a live React 19 /
  TypeScript 6 / Vite 8 dashboard using the unmodified Shadcn neutral component preset, system-aware
  light/dark themes, bounded large-repository tables, and Vitest coverage.

- Added graph and impact `parse_errors` diagnostics. JavaScript, TypeScript, Python, and PHP imports are
  extracted from tree-sitter syntax trees, including re-exports, side-effect imports, dynamic
  imports, bare `require` calls, and Python relative forms, without treating comments, strings,
  or member calls as topology.

- Added changed-line review with `--review` (current findings on changed lines) and
  `--review=deep` (Git base/current snapshot comparison with `new`, `resolved`, `worsened`,
  and `improved` states). It supports `--since`, `--staged`, and `--working`, preserves
  finding identities across detected renames, reads staged content from the index, reports
  through every scan renderer, and can gate CI with `--fail-on-review`.

- Added a complete, versioned canonical finding catalog for complexity violations, precise
  marker occurrences, content-identified duplicate families, and high-risk files. Compact
  `--baseline-ready` JSON retains this catalog while omitting heavy detail arrays, enabling
  finding-level baseline comparison without multi-megabyte artifacts.

- Added `reposcout explain FILE`, which uses a full surrounding-repository scan to explain
  the file's discovery/ignore status, metrics, risk factors, matching tests, direct
  dependencies/dependents, and every related canonical finding. Table, JSON, Markdown, and
  NDJSON outputs are supported.

- Added an ESLint-style per-function cyclomatic complexity rule. Functions and methods
  above `--max-complexity` / `max_complexity` (default `20`) are now counted and exposed
  as precise top-N `complexity_violations` findings in JSON, table, and Markdown; SARIF
  emits every violation for lint/code-scanning workflows.
  Rust closures, Python lambdas, Go function literals, PHP closures/arrows, and existing
  JavaScript anonymous callables are measured as independent scopes with binding-aware names.

- Added this changelog and repository guidance requiring notable changes to be recorded
  newest first as part of the same commit.

### Changed

- Context assessment is now evidence-qualified: reports distinguish a known token fit from an
  unavailable token signal and identify disabled complexity/duplication/churn inputs instead of
  turning their synthetic zeros into confident recommendations. Focus paths resolve against both
  the repository and a nested scan target, with ambiguity and unmatched paths reported explicitly;
  an oversized explicit focus can retain a bounded body-free declaration outline without charging
  its source against the token budget.

- The web graph is now a hierarchical repository explorer. Its architecture view groups mixed
  repositories into deterministic directory/package scopes with breadcrumbs, drill-down, weighted
  aggregate connections, and smooth Bézier routing; file neighborhoods remain bounded to 100
  nodes. Scope, file, and connection inspectors expose factual language mix, topology coverage,
  metrics, risk, churn, symbols, callable complexity, markers, findings, dependencies/dependents,
  cycles, and resolver provenance. Search covers every scanned file, including recognized files
  outside the dependency topology. A single node click selects it and highlights only immediate
  connections; a double-click opens its nested scope or bounded file neighborhood. The explorer
  keeps dense flat-file views readable with left-to-right layout, a legible minimum initial zoom,
  subdued idle edges, selection-driven arrows/labels, and an immediately populated high-contrast
  minimap. It stays local and deterministic without AI, project-tour, or source-opening actions.

- The web dashboard now keeps Graph as its rightmost tab and gives the explorer a larger canvas
  plus an in-app expanded workspace with Escape-to-restore. Curved, selectable import edges
  highlight the active neighborhood and expose resolver provenance, while the inspector now
  combines architecture roles, dependency impact, cycles, scan metrics, churn, risk, markers,
  and canonical findings for files, with dedicated source/target context for connections.

- Rust integration tests are now safe to run with bare `cargo test`: repository Cargo
  configuration serializes the harness, test CLI scans use a hermetic two-worker profile, and the
  former full-repository self-scans use bounded synthetic fixtures instead.

- Git churn now batches cold and unseen non-root commits through one NUL-delimited native
  `git diff-tree` stream, with transparent all-libgit2 fallback. It performs libgit2 rename
  similarity checks only where a tracked path could actually be a rename target, walks children
  before parents when timestamps tie or skew, and persists immutable commit events plus exact
  result views in the OS cache. Repeated scans at an unchanged `HEAD` avoid history traversal
  entirely, while advanced or rewritten histories reuse every still-reachable cached commit,
  analyze unseen commits, and resolve newly relevant rename candidates on demand. `--no-cache`
  disables both the per-file and Git-history caches.

- Replaced the landing page's temporary radar badges with the dedicated RepoScout fox icon,
  including a transparent project asset used consistently in navigation, the footer, the final
  call to action, and the favicon.

- Line and comment metrics now use tree-sitter comment ranges for first-class languages,
  preventing comment delimiters inside strings from corrupting SLOC. The quote-aware generic
  fallback is explicitly exposed through per-file and summary approximation fields.

- Maintainability Index now follows Microsoft's normalized `0..100` formula and interpretation
  bands. Cyclomatic complexity now aggregates independent file scopes, counts Python
  comprehension/guard clauses and modern JavaScript/TypeScript short-circuit paths, and
  cognitive complexity counts direct self-recursion. Type-2 duplication `similarity` is now
  documented as its weighted structured-token score rather than original-text similarity.
  The risk finding-profile version advances because corrected SLOC/complexity inputs can
  change composite risk scores.

- Marker findings for Rust, Python, JavaScript, TypeScript/TSX, Go, and PHP now come only from
  syntax-tree comment nodes, avoiding false positives from identifiers, strings, docstrings,
  templates, and TSX text. Health assessment duplication now considers non-test code only and
  triggers above 15%, while raw duplication metrics remain whole-scan. The test-presence signal
  and risk reasons now explicitly describe filename/inline-test matching rather than coverage,
  with a reduced 1.10 risk multiplier when no match is found; legacy `untested_*` JSON names are
  preserved for compatibility.

- Diff-scoped baseline profiles now record the resolved Git base tree, so ref aliases to the
  same tree compare cleanly while stale or different base trees are rejected. Baseline artifacts
  inside the target are automatically excluded from the comparison scan.

- JSON, NDJSON, and SARIF rendering now returns explicit errors instead of partial output,
  rejects non-UTF-8 report paths, sanitizes repository text in human formats, and percent-encodes
  SARIF artifact URIs. NDJSON summary records now include report identity/profile metadata and
  duplicate records keep `kind: "finding"` while exposing their type as `finding_kind`.

- CLI `--exclude` patterns now extend configuration-file excludes, `--fail-on-regression`
  requires `--baseline`, and bare `--by-dir` no longer consumes the following scan path.

- Rust inline-test detection now uses AST attributes rather than substring matching. Git churn
  follows detected renames, and scoped cache saves preserve entries outside the current subpath
  or diff while complete root scans still prune stale entries.

- Baselines with compatible finding catalogs now report new/resolved/worsened/improved
  findings in addition to aggregate metric deltas. New and worsened findings participate in
  `--fail-on-regression`; older aggregate-only baselines remain usable with an explicit
  unavailable reason for finding comparison.

- SARIF review output now carries deep-review states as SARIF baseline states, while NDJSON
  streams review metadata and one record per review finding. Human reports show the same
  counts and states.

- Baselines now record the effective analyzer/diff/duplication profile, reject incompatible
  comparisons, and include only metrics produced by the active analyzers. Legacy baselines remain
  accepted for default full scans, and compact `--summary` JSON can now be used as a baseline.

- Risk scores now use stable saturation anchors (1,000 SLOC, cyclomatic 100, and 20 commits), so
  tiny subpath/diff scans are no longer labeled large or complex merely for being the largest file
  in scope. Test-presence matching is path/package-aware to avoid cross-package filename collisions.

- Per-file imports now follow the root-dependency contract: Python dotted imports and JavaScript
  package subpaths collapse to their dependency root, while relative/local imports are omitted.

- Complexity presentation is now function-first: human reports lead with the per-function
  rule, analyzed/violation counts, and actionable symbols, while preserving file-level
  aggregates, risk rankings, and Maintainability Index. SARIF warns only on actual
  threshold violations rather than emitting arbitrary top-N functions as notes.

- Interactive scans now continue from the file progress bar into elapsed-time stage
  feedback for git history, tokenization, exact and Type-2 duplication, cache writes,
  aggregation, directory rollups, baselines, dependency graphs, and change impact.

### Fixed

- Compact JSON/NDJSON output no longer erases explicitly requested graph, directory, impact, or
  context query results when `--summary` is used.

- Fixed `reposcout explain` on macOS paths beneath the system `/var` symlink. Explain now
  canonicalizes the Git-root prefix while preserving repository-internal symlinks, preventing a
  harmless system alias from anchoring discovery at `/` and triggering a runaway filesystem scan.

- Fixed landing-page placement across responsive breakpoints: the fox mark now centers by its
  visible alpha bounds, change-impact connectors terminate at their nodes, the mobile duplication
  ring no longer overlaps a statistic, and hero artwork/chips no longer collide with copy or clip
  on common phone, tablet, and laptop widths.

- Fixed duplicate React keys when multiple dashboard finding rows share a canonical fingerprint.

- Replaced the locale-dependent duplicate total on the web dashboard's Tokens card with
  context-budget usage.

- Fixed `Ctrl+C` shutdown hanging while a dashboard SSE connection remained open.

- Removed the web dashboard's tab-height horizontal scrollbar by using the Shadcn tabs list
  directly with a responsive mobile grid.

- Fixed deep review for unborn repositories, rename/deletion line intersections, ignored-parent
  snapshot semantics, and staged/worktree snapshot consistency. Fixed change impact for deleted
  targets whose parent directories no longer exist, and prevented `explain` from following a
  symlink outside repository discovery policy. Explain now locates the repository before checking
  target symlinks, so platform-level ancestors such as macOS `/var` cannot expand a scan to the
  filesystem root.

- Fixed mixed code/block-comment SLOC, Rust/Python catch-all complexity, JavaScript/TypeScript
  re-export imports, grouped Go type symbol counts, strict trailing-newline duplicate coverage,
  and signed decimal/hex exponent tokenization.

- Output files inside the scan target are excluded by exact path, preventing saved JSON/Markdown
  reports from feeding back into later file, token, and duplication metrics. A single-file target
  can no longer be overwritten by using that same path as `--output`.

- CI gates now reject metrics whose analyzer is disabled, `--only` is rejected alongside analyzer
  subcommands instead of being ignored, and early-closing stdout pipelines exit cleanly without a
  broken-pipe error.

- Python dependency/impact analysis now resolves `from . import sibling` forms, including multiple
  and aliased sibling imports.

### Performance

- Declaration outlines no longer change the per-file cache profile when context is toggled.
  Graph-consuming scans cache per-file import, parse, and explicit type-relation source facts and
  reuse them across graph, context, impact, and explain without reopening/reparsing every source.
  Enrichment remains on demand, so ordinary scans and daemon watch refreshes still avoid graph
  extraction.

- Daemon scans are single-flight and run off the async HTTP runtime. Event bursts and repeated
  rescan requests collapse to one pending scan, keeping the API responsive even when duplication
  or Git churn takes substantial time.

- Duplication detection now rejects ineligible format pools before indexing, computes rolling
  powers in logarithmic time, and stores merged line/token intervals instead of expanding every
  covered position. Very large thresholds and coverage ranges therefore remain bounded.

- Type-2 duplication detection now uses rolling rename-invariant fingerprints, borrowed
  identifier mappings, deterministic fast hash maps, and compact token-range overlap
  suppression before report construction. On an uncached 346-file `libc` corpus, this
  reduced runtime from more than 74 seconds to about 6 seconds without lowering the
  candidate budget or similarity threshold.
