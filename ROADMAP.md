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
and complexity. Selected first-class-language files include bounded declaration outlines rather
than source bodies. With a diff scope, changed paths seed a separate full-tree planning universe
while the ordinary scan facts remain change-scoped. The result includes selection reasons,
machine-readable evidence/confidence, payload/timing measurements, and bounded omission
diagnostics.

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

## Delivered priorities (through 2026-07-17)

### Live diagnostics and bounded Type-2 duplication (2026-07-17)

`--debug-log <FILE>` now gives slow or crashing runs an immediately flushed, schema-versioned
NDJSON trace. It covers invocation/configuration, discovery, broad stages, per-file worker timing,
rendering/output, runtime failures, panics, two-second liveness and Linux memory heartbeats, and
rate-limited inner Type-2 progress. The log is excluded exactly from discovery and daemon watcher
feedback and is never allowed to overwrite an existing file.

Dogfooding that trace on a large repetitive repository exposed one JSON pool with 832,934,702
planned seed pairs, linear covered-region lookup, and an unbounded match buffer. Type-2 orchestration
now uses merged diagonal intervals, deterministic rare-first admission, and per-format-pool bounds
of 10,000,000 seed pairs, 250,000 compact matches, and 10,000,000 suppression overlap checks. A
bounded result remains useful but is never mislabeled as complete: live events, JSON diagnostics,
human reports, and capability discovery expose the effective bounds, reasons, and omitted work.
The frozen public detector adapter remains exhaustive; the bounded policy belongs to the reporting
orchestration layer that can carry those diagnostics.

### Agent-efficient CLI contract (2026-07-17)

The CLI now exposes an inexpensive `agent` profile and a guardrailed `safe` profile that ignores
repository-owned configuration, and `--no-project-config` for callers that need an explicit trust
boundary without the other safe limits. Reports identify the effective profile/config sources,
unavailable assessment evidence, cache hits/misses, reusable graph-fact coverage, and coarse stage
timings. `--error-format json` keeps usage and runtime failures machine-readable. Explicit graph,
directory, impact, and context queries survive compact JSON projection instead of being erased by
`--summary`.

`reposcout capabilities -f json` lets automation discover commands, formats, profiles, bounds, and
language coverage without scanning. `reposcout locate SYMBOL [PATH] -f json` provides ranked or
case-sensitive exact declaration lookup across all first-class languages, with kind/language
filters and a hard result bound. A cold lookup performs the configured per-file analysis so it can
populate and then reuse the ordinary scan cache rather than maintaining a second query-only
profile. Focus resolution is now explicit about target-relative paths, ambiguity, and misses; an
oversized explicit focus retains a bounded body-free outline without pretending its source fit the
token budget.

These query paths share cached declaration and on-demand graph source facts with scan, context,
graph, impact, and explain. Ordinary scans and watched daemon refreshes still do not perform graph
extraction; a graph-consuming request enriches missing facts lazily and later requests reuse them.

### Structural context plans

Selected Rust, Python, JavaScript, TypeScript/TSX, Go, and PHP files now carry compact exported/public
and representative declaration headers, signature text without bodies, and a reason for each
symbol. Per-file/aggregate limits, serialized outline bytes, omission counts, and incremental
planning milliseconds make the payload inspectable. Whether this causes agents to open fewer
irrelevant files remains an outcome to evaluate with real task traces rather than infer from the
implementation.

### Change-aware context plans

`--since`, `--staged`, and `--working` now seed context automatically. The planner combines
changed files, direct dependencies, matching tests, direct/transitive dependents, and nearby risk
under the existing hard budget. The normal summary/files/findings stay diff-scoped; only planning
uses full-tree cached facts. Precise direct syntax/config evidence is `high`, while heuristic or
transitive evidence is `partial`. Deleted paths remain virtual topology seeds, and the topology is
shared with `--impact` while `--review` remains independently available.

### Graph precision without overstated coverage

Local JS/TS package export/import maps, entrypoints, subpaths, and runtime-extension substitution
now resolve deterministically; duplicate package names become diagnostics instead of arbitrary
edges. Python absolute imports resolve only when repository/root selection is unambiguous. The
PHP graph resolves Composer `autoload` and `autoload-dev` PSR-4/PSR-0 maps plus static includes;
the zero-index path remains available and every edge names its resolver. Rust module declarations,
local use paths, and Cargo-local crate names now resolve with distinct provenance. Go module-local
package imports resolve to a deterministic package representative, and non-representative package
files are not mislabeled as independent orphan candidates.

### Hierarchical mixed-language graph explorer

The web graph now derives deterministic architecture scopes from repository directories and
package manifests, collapses redundant single-child entry chains, and renders a package or area as
a selectable React Flow parent containing its useful immediate child scopes and files. It
aggregates cross-scope relationships with weights and resolver provenance and supports breadcrumbs
from repository to a bounded file neighborhood without forcing empty intermediate drill-downs.
Smooth Bézier edges, spaced left-to-right file layout, subdued idle topology, and a readable
minimum zoom keep dense connections navigable; the high-contrast minimap is backed by explicit
node dimensions. A single click isolates direct relationships, while double-click enters a scope
or file neighborhood. Files containing highly extended or implemented types grow according to
explicit symbol reach, independently of import fan-in. Opening one of those files centers the
dominant type in a semantic neighborhood, separates incoming/outgoing explicit type relations into
labeled parent groups, and relegates bounded direct imports to quieter context groups; users can
switch back to the unrestricted direction/depth neighborhood without losing the focus. Scope, file,
relationship-group, and connection inspectors expose only existing scan/graph facts—language mix,
topology coverage, metrics, risk, churn, symbols, callable complexity, markers, findings, cycles,
dependencies/dependents, and concrete resolver-backed connections. Search includes scanned
non-topology files. No AI, guided tour, or source-opening feature was added.

Graph navigation is now URL-addressable below `/graph`: architecture scopes and focused file
neighborhoods have readable repository-path routes, non-default presentation/direction/depth state
is canonicalized in the query string, and the existing breadcrumbs drive browser history. Refresh,
bookmark, Back, and Forward therefore restore meaningful graph navigation without treating every
single-click highlight as a new location.

SCIP was evaluated as an optional precision source. It is a good interoperability boundary for
definitions/references that language-specific indexers already computed, but producing an index
is external, potentially expensive, and not universal across repositories. RepoScout therefore
does not silently require or generate SCIP data. A future opt-in consumer should accept a supplied
index, report its provenance/version and stale/missing coverage, and fall back to the current
heuristic topology rather than replacing it.

## Recently delivered

### Change-focused, token-efficient decision reports

**Status:** Released in `v0.1.8`. The public CLI/report contract, scope-aware confidence, bounded
renderers, capability discovery, tests, bundled agent skill, and user-facing documentation are
complete. The change is additive and retains the existing detailed summary/context/impact
workflow.

#### Problem and evidence

RepoScout is intended to answer a scouting question with the smallest trustworthy payload:
identify what matters, explain what may be missing, and let the caller request deeper evidence
only when needed. Before this work, the change-analysis path did not yet meet that standard.

Post-release field feedback from a real 17-file port and Docker Compose change found that
RepoScout was easy to use, found the complete changed-file set, provided a useful reading plan,
and honestly reported graph uncertainty. It nevertheless mixed general complexity, risk, and
repository-health signals into a narrowly scoped change investigation. The report also reduced
impact confidence because the full repository contained 49 parse errors and two unresolved
imports without explaining how many gaps intersected the changed files or known impact
neighborhood. The caller therefore had to treat every graph gap as equally relevant.

The previous behavior followed from two existing contracts:

- `--summary` is currently a serialization-size projection. It removes the heavy `files`,
  `duplicates`, and finding-catalog arrays, but deliberately retains the full aggregate
  `summary`, including general rankings.
- Impact uses a full-tree topology and conservatively reduces one global `confidence` value for
  any unsupported changed file, unresolved import, unreadable graph file, parse error, or
  resolver-configuration error anywhere in that topology.

The conservative behavior is honest, but not decision-efficient. A parse error in a changed file,
one in a known dependent, and one in a distant unrelated-looking file all have different practical
meaning. The distant error can still conceal an undiscovered relationship, so it must not be
silently ignored; it should be presented as a potential repository-wide blind spot rather than as
an unexplained task-local failure.

#### Desired outcome

Add one explicit change-report path that is fast by default, bounded in serialized size, and
organized around the caller's next decision. It should answer, in order:

1. What changed inside the requested target?
2. Which changed files are eligible for first-class graph analysis?
3. Which dependencies, dependents, and matching tests are already known?
4. Which files should be read first?
5. Which coverage gaps directly affect the observed change neighborhood?
6. Which repository-wide gaps could still hide additional relationships?
7. Which evidence-backed validations should the caller consider next?

The ordinary scan and existing `--summary`, `--context`, `--impact`, `--review`, graph, baseline,
and full-report contracts remain available. Concision must be a projection over shared facts, not
a second scanner or a reason to remove detailed analysis.

#### Product principles

- **Progressive disclosure:** the change report is the smallest decision payload; existing
  options remain the path to context outlines, full impact lists, findings, graph topology,
  health rankings, and per-file facts.
- **Relevant precision:** distinguish observed change-scope coverage from whole-repository
  discovery completeness instead of collapsing both into one unexplained label.
- **Conservative honesty:** off-scope parse and resolution gaps remain visible because they may
  conceal an edge into the known change neighborhood.
- **Bounded output:** path lists, gap details, validation hints, and reasons have deterministic
  limits plus omitted counts. Payload growth must not be linear in an arbitrarily large
  repository.
- **No duplicated analysis:** reuse diff discovery, cached file facts, the context planner,
  test matching, graph topology, and impact traversal. The feature may project or aggregate
  those facts but must not build parallel implementations.
- **No hidden verification:** validation entries are recommendations with evidence, never claims
  that a command ran or a behavior passed.
- **No semantic pretense:** a diff identifies a change scope, not the human intent behind it.
  Version one is change-focused; it does not claim to understand an arbitrary natural-language
  task.

#### User-facing CLI contract

Add one scan option:

```text
--change-summary
```

It requires exactly one existing diff scope: `--since <REF>`, `--staged`, or `--working`.
It implies compact rendering, context planning, and impact analysis because all three are part of
the promised decision report. When no explicit `--profile` is supplied, it uses the existing
`agent` profile; `--profile full` and `--profile safe` remain explicit overrides, and the effective
choice stays visible in execution metadata. It does not silently enable duplication or churn.

Examples:

```bash
# Fast, bounded review of uncommitted work.
reposcout --working --change-summary -f json .

# Compare a branch with main while applying untrusted-repository guardrails.
reposcout --since main --change-summary --profile safe -f json .

# Request the existing detailed change-analysis blocks instead of the bounded projection.
reposcout --working --context --impact --summary --profile agent -f json .

# Request health findings and deep snapshot comparison explicitly.
reposcout --since main --review=deep --profile full -f json .
```

The flag supports table, Markdown, JSON, and NDJSON. SARIF remains a findings interchange format,
while DOT and Mermaid remain graph formats; combining any of those three formats with
`--change-summary` fails with one structured usage error. Existing output-file exclusion and
overwrite behavior applies unchanged.

The new flag is a rendering/query contract, not a persistent configuration key in version one.
A repository must not force every caller into a change-only projection. Capability discovery
advertises the option, implied analyses, supported formats, and hard payload limits.

#### Additive report model

Add an optional top-level `change_summary` block to `ScanReport`, guarded with serde defaults and
omitted when the flag is absent. Normal full JSON may serialize the block when explicitly
requested. The bounded `--change-summary` JSON projection identifies itself with
`report_kind: "change-summary"` and retains only the report envelope, analysis/execution metadata
needed to interpret the result, primary scan diagnostics, and this block; it omits the general
aggregate `summary`, file arrays, duplicate arrays, finding catalog, raw context plan, and raw
impact block. Consumers therefore select the dedicated projection model by `report_kind` instead
of trying to deserialize it as a complete `ScanReport`.

The logical contract is:

```text
report_kind                     change-summary
change_summary
  strategy_version
  scope                         since | staged | working
  executive
    changed_files
    graph_eligible_changed_files
    known_direct_dependents
    known_transitive_dependents
    matching_tests
    confidence                  high | partial | none
    reasons[]                   stable reason codes
  changed
    total
    shown
    omitted
    files[]
      path
      graph_eligible
      graph_covered
  reading_order[]
    path
    roles[]                     changed | dependency | dependent | matching-test | nearby
    confidence                  high | partial
    distance?
    resolver?
  reading_order_total
  reading_order_shown
  reading_order_omitted
  impact
    direct_total
    transitive_total
    shown
    omitted
    files[]
      path
      distance
      confidence
      resolver?
  tests
    total
    shown
    omitted
    files[]
      path
      matched_sources[]
      confidence                partial unless syntax-proven evidence is introduced later
  coverage
    observed_scope_confidence   high | partial | none | not-applicable
    discovery_completeness      high | partial | none
    test_mapping_confidence     partial | none | not-applicable
    graph_eligible_changed
    graph_covered_changed
    non_graph_changed
    relevant_gaps
      unreadable_files
      parse_errors
      unresolved_imports
      config_errors
    outside_known_scope_gaps
      unreadable_files
      parse_errors
      unresolved_imports
      config_errors
    gaps[]
      path
      scope                     changed | known-impact | selected-context | outside-known-scope
      unreadable
      parse_errors
      unresolved_imports
      config_errors
    gaps_omitted
  validations[]
    kind
    target?
    reason
    confidence
  validations_omitted
```

This is a logical shape, not permission to duplicate the same path without bounds across every
list. Implementations should use small reusable bounded-list structs when that makes omission
semantics more consistent. Stable reason and validation `kind` values must be documented and
covered by contract tests.

Context declaration outlines are intentionally absent from the change summary. They remain
available through the existing detailed context report and are often the largest useful part of
that block. The concise reading order carries only the evidence required to decide which file to
open.

#### Coverage and confidence semantics

Keep the existing `ImpactAnalysis` fields unchanged for compatibility. Build the more precise
coverage block from the same `Topology` and impact traversal.

Classify changed paths before computing confidence:

- **Graph-eligible changed files** are existing or deleted paths detected as one of RepoScout's
  first-class languages.
- **Graph-covered changed files** are eligible paths represented by a topology node or a supported
  virtual deleted node.
- **Non-graph changes** are recognized content/build formats, unsupported formats, and other paths
  for which RepoScout does not promise import-graph coverage. Report them as intentionally
  non-graph rather than silently treating them as parser failures.

Define the **known impact scope** as graph-covered changed nodes plus every direct or transitive
dependent reached by the reverse traversal. Define the **selected context scope** as the additional
dependencies, matching tests, and nearby files retained by the bounded context plan.

Diagnostics are then partitioned:

- **Relevant gaps** originate in a changed node, known impact node, selected context node, or
  resolver configuration that governs one of those nodes.
- **Outside-known-scope gaps** originate elsewhere in the full topology. They remain potential
  blind spots because a parse or resolution failure may have hidden an edge into the change.
- An unresolved import is attributed to its importer. Parse and unreadable diagnostics already
  have node identity internally and must retain it through projection. Resolver configuration
  diagnostics must gain bounded source-path attribution instead of remaining only one global
  count.

Confidence dimensions have deliberately different meanings:

- `observed_scope_confidence` is `high` only when every graph-eligible changed file is covered and
  relevant graph diagnostics are clean; `partial` when some relevant evidence is missing; `none`
  when graph-eligible changes exist but none are covered; and `not-applicable` when no changed path
  is graph-eligible.
- `discovery_completeness` is `high` only when the full topology has no unreadable, parse,
  unresolved-import, or resolver-configuration gap that could conceal another relationship. It is
  `partial` when useful results exist with such blind spots and `none` when graph construction
  produced no usable changed seed.
- `test_mapping_confidence` is `partial` for the current filename/convention-based matching,
  `none` when eligible source files have no match, and `not-applicable` when the change contains no
  eligible source file. Matching tests are not measured coverage.
- Executive `confidence` is conservative: `high` requires both high observed-scope confidence and
  high discovery completeness; `partial` means the reported neighborhood is useful but incomplete;
  `none` means RepoScout has no graph-backed impact answer. Stable reason codes explain which
  dimension caused the result.

This means a repository with 49 distant parse errors may still report
`observed_scope_confidence: high`, while keeping `discovery_completeness: partial` and executive
`confidence: partial`. The caller learns that known local evidence is clean without being told that
the blast radius is proven complete.

#### Bounded diagnostics and path allocation

The concise report uses a single deterministic path budget so multiple sections cannot each grow
to repository size. Reserve capacity in this order:

1. changed files;
2. relevant coverage gaps;
3. matching tests;
4. direct dependents;
5. selected dependencies and other context files;
6. transitive dependents;
7. outside-known-scope gap examples.

Within one priority, sort by existing context score or graph distance and then normalized path.
Always retain total/shown/omitted counts. Start with a hard aggregate limit of 100 serialized path
entries, a maximum of 25 detailed gap entries, and 10 validation entries. Expose these constants
through `reposcout capabilities -f json`; do not add configuration until real usage demonstrates
that callers need different limits. Existing maximum Git-path-byte rules still apply.

The output-size contract is structural: after metadata and fixed counters, report size is bounded
by these entry limits rather than total repository size. Paths may appear in more than one logical
role only when the additional role changes a decision; renderers should merge roles where possible.

#### Evidence-backed validation guidance

Validation guidance is useful only when it remains factual and narrow. Version one may recommend:

- mapped test files already selected by the existing test matcher;
- validation of a changed manifest, build file, or recognized tool configuration, naming the file
  that triggered the recommendation;
- inspection of changed non-graph files that cannot participate in impact analysis; and
- specialist verification when RepoScout has a relevant graph or parser gap.

Every entry carries a stable kind, target when known, reason, and confidence. An exact shell command
may appear only when it is read directly from already-consumed project metadata through a bounded,
tested parser; otherwise report the validation category and evidence, not an invented command.
RepoScout does not execute the recommendation.

Version one does not infer an old literal from arbitrary diff text, understand a natural-language
task, validate Docker Compose, run Make targets, start services, call health endpoints, or claim
that a selected test is sufficient. External diagnostics can later add stronger failure evidence
to the same context planner, but that separate roadmap item is not a prerequisite for this report.

#### Compatibility and integration

- Invocations without `--change-summary` have byte-for-byte-equivalent report selection semantics;
  existing fields and `--summary` behavior do not change.
- The model change is additive and does not require a `SCHEMA_VERSION` bump. It does not require an
  `ANALYZER_VERSION` bump unless implementation changes cached per-file facts.
- Baseline compatibility remains based on the existing analysis profile. The change summary is a
  query projection and is not baseline input.
- The context and impact blocks remain the detailed source of truth. The concise block must be
  assembled from their shared inputs/results in `scan.rs`, not reconstructed by JSON or human
  renderers.
- Graph node diagnostics need bounded path attribution in the graph module. Renderers consume the
  resulting model and do not inspect topology internals.
- Update capability discovery, CLI/reference documentation, agent workflows, the installed skill,
  and repository agent guidance together so the recommended change-review command uses the new
  concise path.
- Preserve terminal, Markdown, JSON, NDJSON, path-escaping, and control-character safety rules for
  every repository-derived value.

#### Performance and token budgets

`--change-summary` is explicit permission to perform the same on-demand graph/context work as the
current `--working --context --impact` workflow. It must not add another discovery pass, AST parse,
or graph build. Assembly should be linear in the already-built bounded facts plus graph nodes and
edges, with bounded retained detail.

Measure cold/no-cache and warm-cache runtime against the equivalent existing agent-profile change
analysis. The new projection must add no material scan-time regression; use 5% as an investigation
threshold rather than masking noise with a generous budget. Record serialized bytes and a
deterministic token estimate for representative fixtures. A 17-file mixed source/config/docs
fixture should retain every changed path under the default limit while producing at least 60% fewer
serialized bytes than the current summary/context/impact JSON and no general health rankings.

#### Failure behavior

- Missing or conflicting diff scope is a usage error before scanning.
- A valid empty diff returns a successful, minimal report with zero counts, `not-applicable`
  observed/test coverage, and no invented validation.
- No first-class changed file is not an error. List the non-graph changes and report graph impact as
  not applicable.
- A missing/deleted eligible file may remain a virtual changed seed using existing impact semantics.
- Resource truncation, deadline expiry, unreadable input, or bounded omission stays explicit in
  primary diagnostics and lowers the appropriate coverage dimension.
- If concise projection fails after analysis, return the normal structured runtime error; do not
  silently fall back to the much larger full report.

#### Validation and acceptance scenarios

Automated tests must cover:

- CLI requirements, implied agent/context/impact behavior, explicit full/safe overrides, supported
  formats, incompatible formats, capability discovery, and structured errors;
- additive deserialization defaults plus unchanged output selection for every invocation without
  `--change-summary`;
- deterministic path-budget priority, role merging, omitted counts, path normalization, control
  escaping, and stable reason/kind values;
- changed, direct-dependent, transitive-dependent, dependency, matching-test, nearby,
  non-first-class, unsupported, unreadable, and deleted-file cases;
- relevant versus outside-known-scope parse errors and unresolved imports;
- resolver-configuration error attribution to affected files;
- all confidence states and the invariant that repository-wide blind spots prevent a false
  completeness claim without erasing clean observed-scope evidence;
- empty diffs, subpath targets, output-path exclusion, project/global configuration, and safe
  resource truncation;
- table, Markdown, JSON, and NDJSON projections;
- bounded serialized size on synthetic repositories larger than every detail cap; and
- cold/warm timing plus output-byte comparison against the equivalent existing workflow.

Acceptance examples:

- **Given** 17 changed files below the path budget, including source, tests, Compose, Make, and
  documentation, **when** a working-tree change summary runs, **then** all 17 appear as changed,
  only eligible source files participate in graph coverage, and general complexity/risk rankings
  are absent.
- **Given** clean changed and known-dependent nodes plus 49 parse errors outside the known impact
  scope, **when** impact is summarized, **then** observed scope is high, discovery completeness and
  executive confidence are partial, outside-scope totals/examples are visible, and no error is
  mislabeled as directly change-local.
- **Given** a parse error in a changed first-class file, **when** impact is summarized, **then** the
  file is a relevant gap and observed-scope confidence is partial.
- **Given** only Markdown and Docker Compose changes, **when** the report runs, **then** graph
  coverage is not applicable, the files remain in the reading order, and configuration validation
  is recommended without claiming it ran.
- **Given** more paths or gaps than the hard limits, **when** the report renders in every supported
  format, **then** priority order is deterministic, totals remain accurate, and omitted counts make
  truncation explicit.
- **Given** the same invocation without `--change-summary`, **when** JSON is rendered, **then** the
  established summary/context/impact contract remains unchanged.

#### Delivery, rollback, and risks

Ship the additive model and scope-aware coverage tests before switching agent documentation to the
new command. Keep the old recommended command valid throughout. The feature has no persisted data
or migration; rollback consists of removing the new option/projection before a stable contract is
promised, while the underlying diagnostic attribution can remain as an internal improvement.

| Risk | Mitigation |
|------|------------|
| A concise report hides a consequential global gap | Always include repository-wide gap totals, bounded examples, and conservative discovery confidence |
| “Relevant” is mistaken for semantically related to the human task | Name the feature change-focused and define relevance only from diff, graph, test, and context evidence |
| New convenience behavior silently changes analysis cost | Advertise implied context/impact work; reuse one topology; expose timing and the effective profile |
| The projection duplicates context/impact logic and drifts | Assemble it beside shared scan results; keep renderers data-only; add cross-contract tests |
| Path lists become large enough to defeat token savings | Use one aggregate priority budget with omitted counts and capability-advertised hard limits |
| Validation suggestions become hallucinated commands | Require direct metadata evidence for commands; otherwise emit only a category, target, reason, and confidence |
| More confidence labels create confusion | Define each dimension narrowly, include stable reason codes, and render one short executive explanation |
| Compatibility pressure prevents improving `--summary` | Leave existing behavior intact and make the new projection an explicit contract |

This opportunity is **implemented and awaiting release**. Future changes should preserve the
version-one boundaries, progressive-disclosure contract, and conservative confidence semantics
documented above.

## Post-0.1 evidence-gated opportunities

These are possible follow-ups, not committed work or blockers for `v0.1.0`. Reconsider each one
only when post-release usage provides evidence that the current product is insufficient.

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
