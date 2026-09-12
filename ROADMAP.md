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

For the common agent decision, `--agent-summary` now projects those facts into a JSON-only view
with fixed section caps and a hard 16 KiB ceiling. Specialized graph/review/baseline questions stay
on ordinary summary/full output so the default machine contract remains unchanged; see the
[0.2.1 changelog](CHANGELOG.md#021---2026-09-04) for the maintained delivery record.

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

RepoScout's source-first health model, bounded diagnostics, hard-bounded agent view, structural
context plans, mixed-language graph, live dashboard, and progressive skill guidance are
established product behavior. The release-by-release record belongs in `CHANGELOG.md`; this
summary should be removed once it no longer helps explain the remaining roadmap.

## Planned 1.0 compatibility reset

Until the `1.0.0` release, additive defaults and narrowly detected renderer fallbacks keep useful
pre-1.0 reports and baselines readable. The `1.0.0` release must remove pre-1.0-only fallbacks,
defaults, and compatibility tests, require stored reports and baselines to be regenerated, and
document that migration. Compatibility guarantees after that reset start with the stable 1.0
contract; this is not permission to break post-1.0 reports casually.

## Recently delivered

Releases `0.1.8` through `0.2.1` added bounded change decisions and work-scope evidence, compact
JSON, a hard-bounded agent view, production-focused and artifact-filtered duplication, hardened
outputs/releases, the responsive terminal report, and runner-evidence-backed test presence. See
`CHANGELOG.md` for details; remove this paragraph once those contracts are ordinary background.

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

Delivered in [0.3.0](CHANGELOG.md#030---2026-09-13); the maintained contract is now in
[task diagnostics](docs/task-diagnostics.md). Broader efficiency evidence remains unresolved as
recorded in the [historical pilot](docs/agent-evaluation.md).

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
