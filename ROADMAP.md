# RepoScout research and roadmap

_Research refreshed: 2026-07-29_

RepoScout's north star is a fast, local-first repository intelligence layer for humans and
agents: one command should reveal what matters, what can be skipped, what fits in a context
window, and what a change could affect. It should help a caller decide what to do next without
copying source into an opaque service or building a heavyweight index first.

This document records the competitive research behind that direction and the highest-value
follow-ups. It is a roadmap, not a promise that every idea will ship.

## What adjacent tools teach us

| Tool | Distinct strength | Lesson for RepoScout |
|------|-------------------|----------------------|
| [Aider repository map](https://aider.chat/docs/repomap.html) | Fits ranked symbols and signatures into a token budget, using dependency-graph importance to decide what enters the map. | Token budgets and structural importance are excellent agent-facing primitives. RepoScout should explain its ranking and keep the plan separate from source packing. |
| [Repomix](https://repomix.com/guide/configuration) | Packs a repository into an AI-friendly artifact, with Tree-sitter compression, token counts, Git context, and security checks. | Packing is useful downstream, but RepoScout's earlier decision point is choosing the smallest trustworthy reading set. An explicit plan can feed a packer without duplicating it. |
| [jscpd](https://jscpd.dev/getting-started/introduction) | Dedicated clone detection across many formats, with machine reports and a Rust-based performance focus in v5. | Keep duplication evidence precise and actionable, while differentiating through the combined risk, test, graph, churn, and context view. |
| [Sourcegraph SCIP](https://sourcegraph.com/docs/code-navigation/writing-an-indexer) | A language-agnostic protocol for precise definitions, references, and implementations produced by language-specific indexers. | Precise cross-language navigation is valuable but expensive to reproduce. Future interoperability with existing indexes is more credible than pretending heuristic imports are exact. |
| [dependency-cruiser](https://github.com/sverweij/dependency-cruiser) | Dependency validation rules, affected-module queries, and multiple graph formats for JavaScript/TypeScript. | Graphs become more useful when they answer a decision question. Prioritize blast radius, architecture violations, and explainable context selection over decorative graph output. |
| [Madge](https://github.com/pahen/madge) | Focused dependency graph, circular dependency, dependent, and orphan queries. | Small, direct graph questions deserve stable machine-readable answers and honest resolution diagnostics. |
| [ast-grep](https://ast-grep.github.io/guide/introduction.html) | Fast structural search, lint, and rewriting over Tree-sitter syntax. | RepoScout can expose structural scouting signals, but should integrate with specialist search/refactor tools instead of growing a rewrite engine. |
| [Tokei](https://github.com/XAMPPRocky/tokei) and [scc](https://github.com/boyter/scc) | Very fast language and code statistics over large trees; scc also exposes complexity-oriented signals. | A near-zero-friction default path is part of the product, not an implementation detail. Every new whole-repository feature needs an opt-in or bounded design. |

Configuration behavior was also informed by mature command-line tools:

- [Ruff](https://docs.astral.sh/ruff/configuration/) makes effective settings inspectable and
  uses explicit precedence between command-line and configuration values.
- [Cargo](https://doc.rust-lang.org/cargo/reference/config.html) uses hierarchical configuration
  where the closer project context wins.
- [Aider](https://aider.chat/docs/config/aider_conf.html) layers home, repository, and working
  directory configuration so personal defaults can coexist with committed team policy.

## Current decision: a context plan, not a source bundle

RepoScout now has an opt-in, deterministic context planner. Given a hard token and file budget,
it ranks analyzed files using focus paths, same-directory siblings, supported-language graph
relationships, matching tests, repository instructions, manifests, entry points, risk, churn,
and complexity. Full context reports include bounded declaration outlines rather than source
bodies; summary projections retain their counts and ranked file evidence but omit the declaration
objects. With a diff scope, changed paths seed a separate full-tree planning universe while the
ordinary scan facts remain change-scoped. The result includes selection reasons, machine-readable
evidence/confidence, payload/timing measurements, and bounded omission diagnostics.

This is deliberately a planning contract:

- It does not copy source into the report, send data over a network, use embeddings, or call a
  model.
- It never exceeds the requested aggregate token budget or file limit.
- Structural outlines have independent per-file and aggregate byte/symbol bounds.
- Recognized non-first-class languages do not receive invented dependency confidence.
- Change-aware plans reuse discovery, per-file caches, Git diff semantics, and the same graph
  topology as impact analysis instead of creating parallel implementations.
- It remains opt-in so ordinary scans keep their existing performance profile.

The matching configuration foundation layers built-in defaults, an OS-appropriate global file,
the nearest project `reposcout.toml`, and command-line flags. Teams can commit project policy;
individuals can retain global defaults; `reposcout config` shows exactly which sources and keys
won.

The graph is now a decision interface rather than only an aggregate. Full machine reports expose
stable adjacency and edge records; bounded focus queries answer dependency and blast-radius
questions; DOT/Mermaid provide zero-dependency human exports. Every first-class language can
participate in the same mixed-repository topology. JS/TS resolution consumes
`tsconfig.json` / `jsconfig.json` path mappings, local `package.json` exports/imports/entrypoints,
and runtime-extension substitution to checked-in TypeScript. Python resolution covers relative
and unambiguous absolute imports, including conventional `src/` roots. PHP resolution covers
Composer PSR-4/PSR-0 namespace maps and static include/require paths. Rust resolution covers
modules, local use paths, and Cargo-local library names; Go module imports point to a stable
package representative so that package evidence is not mislabeled as precise file evidence.
Syntax-proven `extends`, `implements`, trait, and embedding relationships form a separate symbol
topology across every first-class language; ambiguous names remain unresolved rather than being
folded into import fan-in. Every edge records resolver provenance, and configuration errors remain
explicit. This follows Madge and dependency-cruiser's useful query/export surface while preserving
RepoScout's local, deterministic, diagnostic-first contract. The daemon dashboard exposes the same
topology only on demand: one cached build per report revision feeds a searchable hierarchy of
architecture scopes, bounded file neighborhoods, and factual scope/file/connection inspectors
with a hard 100-node render bound.

## Delivered foundation

RepoScout's source-first health model, bounded diagnostics, agent-efficient CLI, structural context
plans, mixed-language graph, live dashboard, and progressive skill guidance are established product
behavior. The release-by-release record belongs in `CHANGELOG.md`; this summary should be removed
once it no longer helps explain the remaining roadmap.

## Planned 1.0 compatibility reset

Until the `1.0.0` release, additive defaults and narrowly detected renderer fallbacks keep useful
pre-1.0 reports and baselines readable. The `1.0.0` release must remove pre-1.0-only fallbacks,
defaults, and compatibility tests, require stored reports and baselines to be regenerated, and
document that migration. Compatibility guarantees after that reset start with the stable 1.0
contract; this is not permission to break post-1.0 reports casually.

## Recently delivered

Releases `0.1.8` through `0.1.15` added bounded change decisions and work-scope evidence, compact
JSON, production-focused and artifact-filtered duplication, hardened outputs/releases, and the
responsive terminal report. See `CHANGELOG.md` for details; remove this paragraph once those
contracts are ordinary background rather than useful roadmap context.

## Evidence-gated opportunities

These are possible follow-ups, not committed work or release blockers. Reconsider each one only
when usage provides evidence that the current product is insufficient.

### Explicit exhaustive/high-budget Type-2 mode

The fast default should remain bounded: the pathological dogfood pool completed in about 28 seconds
instead of projecting into days, and ordinary repositories below the bounds retain complete
results. Some workflows nevertheless need maximum Type-2 recall—for example a dedicated duplicate
audit, ground-truth validation of the bounded ranking, or a release baseline intended to measure
every near-clone family.

If real workflows demonstrate that need, add an explicit effort policy rather than silently
escalating. The final CLI shape should be validated before implementation, but it should support
a discoverable high-budget and/or `exhaustive` choice alongside the current bounded default.
Requirements:

- never make an ordinary `reposcout [PATH]` invocation exhaustive, and never infer permission from
  repository size, a TTY, CI, or the presence of a debug log;
- expose the effective effort policy and numeric bounds through capabilities, execution/profile
  metadata, debug events, and config inspection, with CLI settings taking normal precedence;
- make bounded and exhaustive reports baseline-compatible only when their Type-2 effort policies
  match, so a lower-bound percentage is not compared as though it were exhaustive;
- retain `type2_analysis_partial` and omitted-work accounting for any finite high-budget run that
  still reaches a bound; only claim exhaustive when every candidate and suppression result was
  processed;
- improve or replace the current pairwise overlap-suppression index, or use a bounded spill/stream
  strategy, before removing its guardrail. “Exhaustive” must not merely exchange a quick partial
  result for an avoidable out-of-memory failure;
- verify bounded-vs-exhaustive recall on deterministic synthetic repetitive corpora and recorded
  large-repository shapes, while keeping routine tests small and resource-bounded; and
- preserve the frozen `fuzzy::detect` adapter and implement effort policy in duplication
  orchestration, where completeness diagnostics can reach every reporter.

An explicit exhaustive run may legitimately take a long time. Its contract is honesty and
observability, not a universal runtime promise; recommend `--debug-log` for such runs and keep the
live phase/rate/memory counters active.

### Additional policy profiles when workflows justify them

The built-in `agent` and `safe` execution profiles cover cheap scouting and untrusted-repository
operation. Consider named, composable team profiles such as `ci` and `review`, plus scoped
overrides for paths or languages, only after real usage demonstrates repeated configurations.
Any expansion must retain inspectable effective values and unambiguous precedence; hidden merging
would make automation less trustworthy.

### External diagnostics as context-planning evidence

**Status:** Design-ready, medium-risk, post-0.1 opportunity. Implementation remains evidence-gated:
build and test failures must prove that path-only focus and diff-seeded context repeatedly leave
agents without the right reading set. The design is complete enough to implement without relying
on prior conversation or a separate product decision.

#### Problem and desired outcome

RepoScout currently plans context from explicit paths, changed files, repository structure, tests,
graph relationships, risk, churn, and complexity. A compiler, test runner, linter, or security
scanner often has stronger task-specific evidence: one or more concrete source locations that
failed. Agents must currently parse that output themselves, translate locations into repository
paths, and separately ask RepoScout for related files.

The desired outcome is an opt-in, one-shot CLI input that turns external diagnostics into
high-confidence seeds for the existing context planner. RepoScout should normalize the bounded
input, resolve locations against the scanned repository, select the failing files plus their
matching tests or sources and graph neighborhood, and explain every selection. It must not run the
external tool, embed unbounded logs, create a second context implementation, or treat third-party
findings as RepoScout health findings.

Success is observable when:

- a diagnostic with a valid repository location reliably selects that file ahead of generic
  context candidates;
- a source diagnostic brings in matching tests, and a test diagnostic brings in the matching
  production source when the existing filename/test heuristics can establish one;
- supported-language dependencies and direct/transitive dependents are ranked through the same
  graph facts and provenance used by ordinary context planning;
- unresolved, out-of-scope, malformed, and truncated input is explicit in machine and human
  output rather than disappearing;
- the resulting plan remains deterministic and within the existing token, file, outline, scan,
  and graph bounds; and
- ordinary scans without diagnostic input have no behavior or performance change.

#### User-facing contract

Add two scan options rather than a standalone `diagnose` subcommand:

```text
--task-diagnostics <PATH|->
--task-diagnostics-format <auto|sarif|rustc-json|text>
```

`--task-diagnostics` accepts exactly one regular file or `-` for stdin and implies `--context`,
just as `--focus` does. `--task-diagnostics-format` defaults to `auto` and requires
`--task-diagnostics`. Keeping the input on the scan command is deliberate: reformatting a build
log alone is not RepoScout's job; connecting failure evidence to cached repository facts is.

Examples of the intended interface:

```sh
# Structured Rust compiler output.
cargo check --message-format=json |
  reposcout --task-diagnostics - --task-diagnostics-format rustc-json \
    --context --summary -f json .

# A multi-tool SARIF result file.
reposcout --task-diagnostics results.sarif --context --summary -f json .

# A bounded best-effort parser for ordinary build/test output.
cargo test 2>&1 |
  reposcout --task-diagnostics - --task-diagnostics-format text \
    --context --summary -f json .

# Diagnostics can complement, but do not replace, diff evidence.
cargo check --message-format=json |
  reposcout --task-diagnostics - --working --context --impact --summary -f json .
```

Version one is CLI-only. It does not add diagnostic upload, persistence, or watch behavior to the
daemon; does not change `explain`, `locate`, or the web application; and does not allow project
configuration to name a diagnostic file. A task-specific input path is caller-owned authority and
must be supplied explicitly for each invocation.

#### Supported input formats

The parser produces one normalized in-memory contract regardless of input:

```text
TaskDiagnostic
  id                 deterministic report-local identifier
  path               normalized repository-relative path when resolved
  original_path      bounded input path, retained only when unresolved
  line/column        optional one-based start position
  end_line/end_column optional one-based end position
  severity           error | warning | note | info | unknown
  code               optional bounded rule or compiler code
  tool               optional bounded producer name
  message            normalized single-line message
  confidence         high | partial
```

The stable report contract should use normal serde defaults and omission rules so this remains an
additive `SCHEMA_VERSION = 1.0` change. `id` values are assigned after deterministic sorting and
deduplication; they are stable within equivalent input but are not cross-report fingerprints.
Messages, tool names, codes, and original paths must have independent length limits. ANSI and
terminal control characters are removed or escaped before human rendering, and the original
multi-line rendered diagnostic is never copied into the report.

Supported formats:

1. **SARIF 2.1.0 (`sarif`, high confidence).** Read every bounded
   `runs[].results[]` location from
   `physicalLocation.artifactLocation.uri` and `region`, retain `ruleId`, map SARIF levels to the
   normalized severity, and use the run tool driver name as `tool`. Accept repository-relative
   paths and local `file:` URIs; reject network URI schemes. Multiple physical locations are
   separate diagnostic records.
2. **Rust compiler/Cargo JSON (`rustc-json`, high confidence).** Read NDJSON records whose
   `reason` is `compiler-message`, retain the compiler level, code, and message, and create one
   diagnostic for each primary span. A message without a primary span contributes to parse
   accounting but cannot seed context. Do not retain Cargo artifact/build-script records or the
   compiler's rendered multi-line text.
3. **Conservative text extraction (`text`, partial confidence).** Recognize bounded occurrences
   of `path:line:column`, `path:line`, Rust-style `--> path:line:column`,
   `path(line,column): ...`, and Python-style `File "path", line N`. Attach severity, code, and a
   nearby message only when a small fixed window establishes them; otherwise use `unknown` and the
   matched line. Text parsing is location extraction, not an attempt to understand every compiler
   or test framework.

`auto` inspects a bounded prefix without consuming stdin twice. A SARIF object with `version` and
`runs` selects `sarif`; NDJSON with a valid `compiler-message` record selects `rustc-json`; all
other non-JSON input selects `text`. Input beginning with JSON syntax but not matching a supported
structured contract is an unsupported-format error rather than a silent text fallback.

#### Path resolution and trust boundary

Diagnostic content is untrusted even when the repository is trusted. Resolution must reuse the
context planner's normalized path identity and the full planning file universe; it must not create
another filesystem walk. Resolve each location in this order:

1. strip a local `file:` URI, percent-decode it, normalize separators, and reject NUL/control
   characters or a path longer than the configured hard path bound;
2. for an absolute path, accept it only when it resolves lexically and canonically inside the
   repository root;
3. for a relative path, try an exact repository-relative match, then an exact target-relative
   match; and
4. otherwise record the location as unresolved. Do not guess by basename or choose among suffix
   matches.

Only diagnostics inside the requested scan target become seeds. Valid repository paths outside a
subpath target are counted as `out_of_scope`; the input does not silently widen the primary scan.
Existing diff-scoped context behavior remains the sole exception: its already-defined full-tree
planning universe may select unchanged related files elsewhere in the repository. Symlinks do not
authorize paths outside the root, missing/generated paths remain unresolved, and an unresolved
path never becomes a graph seed.

The diagnostic input file itself may live outside the repository because it is explicit
caller-owned input. RepoScout opens it read-only, performs no network access, invokes no shell or
build command, and never persists its contents in the analysis cache.

#### Resource limits and partial results

Use compile-time absolute bounds rather than project-configurable values for the first version:

| Limit | Normal profiles | `safe` profile |
|---|---:|---:|
| Input bytes read | 8 MiB | 1 MiB |
| Parsed diagnostic records | 1,000 | 250 |
| Serialized diagnostic details | 100 | 50 |
| Normalized message | 512 Unicode scalar values | 512 |
| Tool or rule code | 128 Unicode scalar values | 128 |

Read files and stdin incrementally up to one byte beyond the effective input limit so truncation
is detectable without buffering an unbounded stream. Stop parsing after the record limit, retain
the useful prefix, and set explicit `input_truncated`, `records_truncated`, omitted-record, byte,
parse-error, unresolved, and out-of-scope counters. A partial input is a successful scan when at
least one supported record was parsed; the context evidence is useful but must advertise partial
coverage. If a selected structured format yields only malformed records, return a structured CLI
error rather than an apparently clean plan.

These limits belong in `capabilities -f json` and the `safe` profile's disclosed safety limits.
They do not need a configuration surface until real use demonstrates that the fixed normal bound
is insufficient.

#### Context-planner integration

Parsing and path resolution happen before context planning, after the repository root and target
are known. Pass normalized resolved diagnostics through the existing context module seam together
with focus/change seeds; do not let the parser read analyzed source or build graph facts itself.

Required direct-seed ranking invariants, before independent graph/risk/support evidence is added:

- all else equal, explicit `--focus` remains the strongest caller intent;
- all else equal, a resolved error diagnostic ranks ahead of an ordinary changed-file seed;
- all else equal, a changed-file seed ranks ahead of warning/note/info diagnostics when no
  explicit focus also applies;
- every resolved diagnostic is still a seed, overrides generated/minified skip hints, and receives
  an `outline_only` entry when its source cannot fit the token budget but an existing bounded
  declaration outline is available;
- additional diagnostics on the same file add only a capped boost so repeated errors cannot crowd
  out the rest of the plan;
- severity and structured-vs-text confidence break ties before the existing token/path ordering;
  and
- no diagnostic may bypass `context_budget`, `context_max_files`, outline limits, graph depth, or
  scan resource limits.

Implement these invariants with a context strategy-version bump and deterministic tests; exact
numeric weights remain an internal strategy detail. The union of focus, change, and diagnostic
paths seeds the existing dependency/dependent traversal. Reasons must name the actual seed type:
`compiler diagnostic at 42:17`, `matching test for diagnostic source`, `matching source for
diagnostic test`, `direct dependency of diagnostic`, or `dependent of diagnostic`.

Extend `ContextEvidence` additively with an optional list of report-local task-diagnostic IDs.
Direct diagnostic files use role `diagnostic`, distance `0`, and the parser confidence. Related
files retain the existing `matching-test`, `dependency`, and `dependent` roles and reference the
diagnostic IDs that caused the relationship. Add `matching-source` for the reverse test-to-source
case. The path/test matching and graph resolver provenance must come from existing shared facts;
do not add a diagnostic-only import resolver or test matcher.

Version one does not attempt precise line-to-enclosing-symbol resolution. Selected first-class
files receive the same bounded declaration outlines they receive today, while the exact diagnostic
line remains available in task evidence. A later symbol-level enhancement requires independent
evidence and must reuse parser ranges rather than introducing a second symbol index.

#### Report behavior and compatibility

Add `task_evidence` beneath `context`, not at the top level: the existing top-level `diagnostics`
means scan coverage and must keep that meaning. The new block contains:

```text
format
bytes_read
parsed_records
deduplicated_records
resolved_records
unresolved_records
out_of_scope_records
parse_errors
input_truncated
records_truncated
omitted_records
diagnostics[]          bounded normalized details
```

`--summary` retains this block because explicitly requested context blocks already survive compact
projection. `--baseline-ready` continues to remove context and therefore removes task evidence.
External diagnostics do not alter summary metrics, risk, the finding catalog, regression gates,
baseline compatibility, or SARIF results; they are routing evidence, not findings rediscovered by
RepoScout. They also do not require an `ANALYZER_VERSION` bump because no cached `FileReport` fact
changes. The context strategy version must change because selection order changes when the option
is present.

Table and Markdown reports show one compact line with format, parsed/resolved/unresolved counts,
and truncation state, followed by the normal context plan with diagnostic reasons. JSON and NDJSON
carry the full bounded block. DOT/Mermaid output remains graph-only. Debug logs may record input
format, byte/record counts, duration, and truncation, but never diagnostic messages or original
log lines.

`capabilities -f json` must advertise the option, supported formats, normal/safe byte and record
limits, and that task diagnostics imply context. Documentation must distinguish task diagnostics
from top-level scan diagnostics and show at least one SARIF, rustc JSON, and text example.

#### Failure behavior

- Missing, unreadable, or non-regular input is a normal structured CLI error and no scan starts.
- `-` may be used only once and is rejected when stdin is a terminal with no piped data, avoiding
  an accidental indefinite wait.
- An empty valid input succeeds with zero parsed records and an explicit empty-evidence summary;
  this represents a tool run with no diagnostics.
- An explicitly selected structured format with malformed content fails if it produces no valid
  record. Mixed valid/malformed records succeed with `parse_errors > 0`.
- Unknown JSON in `auto` mode fails with an unsupported-format error and suggests
  `--task-diagnostics-format text` only when the caller intentionally wants heuristic parsing.
- All renderer paths escape repository-controlled paths, messages, tool names, and rule codes
  using their existing terminal/Markdown/SARIF-safe primitives.
- A scan or planning limit may still make the repository analysis partial independently of the
  diagnostic input. Both coverage domains remain visible: top-level `diagnostics` describes scan
  coverage; `context.task_evidence` describes external-input coverage.

#### Explicit non-goals

- Running, discovering, or recommending build/test/lint commands.
- A `doctor`, `check --run`, task runner, shell, process supervisor, or CI service.
- Replacing `rg`, compiler-native JSON, SARIF producers, language servers, or test-framework
  reporters.
- Persisting diagnostic logs or normalized task evidence in RepoScout's cache.
- Adding raw source snippets, complete stack traces, compiler-rendered output, or full logs to
  reports.
- Treating external diagnostics as RepoScout findings, changing health scores, or making
  `--fail-on` gates depend on third-party results.
- Diagnostic-seeded standalone `--impact` semantics in version one. When a diff scope is present,
  the existing impact block remains diff-seeded; diagnostics only enrich the context plan.
- Daemon ingestion, web upload, live log following, editor integration, or format-specific plugin
  infrastructure.
- Exact semantic blame, reference lookup, or enclosing-symbol resolution from a line number.

#### Validation and acceptance scenarios

Automated validation must cover:

- fixture parsers for SARIF, rustc/Cargo NDJSON, every supported text location form, empty input,
  mixed malformed records, ANSI/control characters, oversized fields, and invalid JSON;
- deterministic sorting, deduplication, severity mapping, multi-location results, primary Rust
  spans, and auto-detection without rereading stdin;
- absolute, repository-relative, target-relative, percent-encoded, ambiguous, traversal,
  symlink-escape, missing, out-of-root, and out-of-scope paths;
- byte and record limits for file and stdin input, including a useful partial result and accurate
  omitted/truncation counters;
- direct diagnostic ranking, capped repeated-diagnostic weight, generated-file override,
  matching-test and matching-source selection, graph neighbors with resolver provenance, and
  outline-only behavior under a tiny token budget;
- coexistence with explicit focus and `--working`/`--since`, including the ranking invariants and
  preservation of existing diff-seeded impact semantics;
- serde compatibility with reports lacking the additive fields, unchanged baseline/profile
  behavior, compact summary retention, and all applicable renderers;
- arbitrary bounded text never panicking or allocating beyond the declared input/field limits;
  and
- a process-level test proving that task-diagnostic parsing invokes no external command and writes
  no task evidence to the repository or analysis cache.

Acceptance scenarios:

- **Given** one high-confidence Rust error inside a source file, **when** diagnostic context is
  requested, **then** the source is selected with its location, its matching test is ranked, and
  supported graph neighbors cite the same diagnostic ID.
- **Given** a failing test location, **when** the filename heuristic identifies one production
  source, **then** both appear with distinct `diagnostic` and `matching-source` evidence.
- **Given** duplicate text stack frames and hundreds of repeated errors in one file, **when** the
  record limit is not reached, **then** records deduplicate deterministically and the file's score
  receives only the capped repeated-error boost.
- **Given** a path outside the repository or scan target, **when** input is parsed, **then** it is
  counted as unresolved or out of scope and never influences graph or context selection.
- **Given** more input than the effective safe limit, **when** at least one earlier diagnostic is
  valid, **then** the scan succeeds with a useful bounded plan and explicit partial-input state.
- **Given** no task-diagnostic option, **when** any existing invocation runs, **then** serialized
  output, ranking, cache behavior, performance, and exit semantics remain unchanged.

#### Delivery, rollback, and risks

Implement in vertical slices: normalized model and bounded reader; SARIF parser; rustc JSON parser;
text parser; path resolution; context integration; renderers/capabilities/docs. Keep each parser
behind one shared normalization interface and reuse existing `serde_json`, regex, path, context,
graph, and test-matching facilities before adding dependencies. No data migration or cache cleanup
is required.

The feature is additive and opt-in, so rollback is removal of the CLI options and additive report
fields before a release, or a follow-up release that stops advertising them. Once released, keep
deserializing the additive fields even if ingestion is temporarily disabled.

Principal risks and mitigations:

| Risk | Mitigation |
|---|---|
| Untrusted logs cause memory/time exhaustion | Streaming reads, absolute byte/record/field bounds, safe-profile clamps, no raw rendered payload |
| Paths disclose or escape outside the repository | Strict root/target resolution, no basename guessing, reject network URIs and external canonical paths |
| Heuristic text creates false relevance | `partial` confidence, conservative patterns, deterministic unresolved accounting, structured formats preferred |
| Repeated diagnostics dominate the plan | Deduplicate records and cap per-file score contribution |
| Third-party findings are mistaken for RepoScout findings | Keep them under `context.task_evidence`; exclude them from findings, gates, baselines, health, and SARIF output |
| Parser proliferation turns RepoScout into a log framework | Ship only SARIF, rustc JSON, and conservative text; require evidence before adding another format |
| Sensitive log messages leak into debug/cache output | Never cache evidence or log messages; bound and escape only the normalized message in the explicit report |
| Context behavior silently changes for existing users | Opt-in activation, strategy-version bump, unchanged no-input golden tests and benchmarks |

This opportunity is **ready for independent review and implementation planning** once usage
evidence justifies prioritizing it. There are no blocking product questions in the version-one
contract above.

### Historical decision signals

Extend single-baseline comparison into bounded trend summaries: risk movement, duplicate debt,
complexity regressions, and hotspots over selected Git points. Avoid persistent repository-local
state by default, and cap history work so the feature cannot surprise an interactive caller.

### Additional distribution channels

The initial release channel is GitHub Releases. crates.io and Homebrew are deferred until real
usage justifies the packaging, automation, and ongoing maintenance for those channels; do not
advertise `cargo install` or `brew install` until the corresponding distribution is supported.

## Explicit non-goals

### MCP

RepoScout will not add an MCP server or protocol dependency. Stable CLI JSON/NDJSON, capability
discovery, structured errors, guardrailed profiles, and task-oriented queries are the integration
contract. New agent-facing needs should extend those shared CLI and library interfaces instead of
introducing a required daemon, network service, or parallel implementation.

## Product guardrails

- Default scans must stay fast; benchmark changes on warm and cold/no-cache paths.
- Heavy whole-corpus work is opt-in, cached, bounded, or all three.
- Machine output is additive and versioned; confidence and scan gaps are data, not prose-only
  caveats.
- A recommendation must include evidence an agent can act on.
- Local source remains local unless a user explicitly chooses an external integration.
- Prefer deep modules and reusable analysis facts over parallel implementations of discovery,
  parsing, graphing, or Git semantics.

## How to judge new ideas

A feature belongs in RepoScout when it measurably improves at least one scouting decision:

1. Which files or symbols should be read first?
2. What should be skipped?
3. Does the relevant material fit the available context?
4. Where is risk or cleanup value concentrated?
5. What tests and dependents constrain a change?
6. Did the scan have enough coverage to trust the answer?

If an idea primarily edits code, bundles entire repositories, hosts source, or replaces a
language-specific indexer, RepoScout should usually integrate with that specialist instead.
