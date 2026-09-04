# Agent report and mode contracts

This is a normative extension of the root [`AGENTS.md`](../../AGENTS.md). Read it completely before
changing report shapes, summary/change/review/baseline behavior, CLI task queries, or renderers.
The root instructions remain in force.

## Agent summary, summary, and baseline-ready reports

`--agent-summary` is the smallest general agent-scouting contract. It is a pure JSON projection
over the existing `ScanReport`, defaults to the `agent` profile unless explicitly overridden, and
never changes blank or ordinary machine-output behavior. It retains compact interpretation,
coverage, inventory, assessment, health rankings, and optional context decision facts under fixed
per-section caps plus a hard 16 KiB newline-terminated JSON-record ceiling. Every projection-capped
ranking/context tier has exact projection-local `available`/`shown`/`omitted` counts, and context
tiers retain the corresponding token totals. Context output must retain seed/graph coverage totals;
analyzer-specific partiality is emitted only for analyzers that ran; graph diagnostics are emitted
only when a graph consumer ran. Scan, context-budget, analyzer, and baseline completeness stay
separate. If path lengths force further reduction, remove whole entries in a fixed deterministic
order and update counters; never truncate JSON bytes.

Context tiers preserve evidence semantics: without a focus or change seed, the direct tier is
empty and general ranked candidates remain expansion rather than fabricated direct evidence.
Oversized explicit seeds remain in the bounded outline-only tier, which consumers inspect before
the direct and expansion tiers.

The mode is JSON-only and rejects pretty-printing or explicitly requested detailed directory,
baseline, graph, impact, review, snippet, and duplicate-pair blocks. Keep those workflows on the
ordinary summary/full contracts. Agent-summary construction must remain in
`report/agent_summary.rs` and must never trigger analysis, topology, or a second query pipeline.

`--summary` is the general compact aggregate mode. With `-f json --summary`, heavy `files[]`, raw
`duplicates`, and canonical finding arrays are omitted while aggregate `summary` and explicitly
requested context, directories, graph, impact, baseline, and review blocks remain. Keep the
redundancy-filtered `summary.top_duplicates` and optional `top_production_duplicates` so the result
remains actionable. Human table and Markdown already use the production projection; the flag
primarily changes JSON.

Summary JSON remains valid aggregate baseline input because baseline loading consumes report
metadata and `summary`, not omitted arrays. It cannot provide finding-level comparison.
`--baseline-ready` is the finding-complete compact artifact: it removes heavy arrays and opt-in
analysis blocks but retains `finding_catalog`.

Schema 2 reports do not expose inferred source-to-test matches through summaries, directory
rollups, baseline metrics, risk entries, or `explain`. Explain's repository source count is complete
inventory and remains available without a configured runner; its configured test-file count is
optional, and file classification is `unavailable` when no evidence-scoped runner applies.

## Change summary

`--change-summary` is the bounded change-decision mode:

- It requires exactly one of `--since`, `--staged`, or `--working`.
- It defaults to the `agent` profile unless a profile was explicitly selected.
- It implies context and impact analysis.
- Strategy 2 preserves explicit focus evidence as a high-confidence `focus` role in the merged
  reading order.
- JSON/NDJSON identifies `report_kind: "change-summary"` and retains interpretation metadata,
  diagnostics, `work_scope`, and the additive `change_summary` projection. It never includes the
  ordinary aggregate, per-file facts, finding catalog, or raw context/impact blocks.
- Keep the aggregate limits of 100 paths, 25 gaps, and 10 validation entries synchronized with
  capability discovery and the bundled skill.
- Confidence must distinguish clean observed-scope evidence from repository-wide discovery blind
  spots. Matching tests and validation entries are recommendations, never measured coverage or a
  claim that a command ran.

## Canonical findings and review

`findings::build` is the shared projection for every complexity violation, precisely located
marker, duplicate family, and risk score at least 0.7. The top-level `finding_catalog` is versioned
and uncapped.

Path-sensitive fingerprints use semantic function/marker identities. Duplication-family
fingerprints are content-derived and path-independent. Baseline and deep-review comparisons have
exactly four states: `new`, `resolved`, `worsened`, and `improved`. Only new and worsened findings
participate in regression gates.

Review is based on Git content, not just changed filenames:

- Bare `--review` filters current complexity, marker, and duplication findings to zero-context
  changed-line ranges.
- `--review=deep` analyzes both snapshots through `scan::analyze_source`. Staged current content
  comes from the index, other current content from the worktree, and base content from the selected
  ref or `HEAD`.
- Both snapshots honor current discovery policy. Duplication runs across each complete snapshot so
  a changed fragment can match unchanged code. Git-detected renames are remapped before comparison.
- `--fail-on-review` gates all fast findings, but only deep `new` and `worsened` states.

## Work scope and opt-in blocks

`work_scope` is bounded raw evidence, not agent routing. Strategy `2` reports primary inventory,
production-source duplication evidence when available, focus/diff seeds, context selection and
uncapped omission totals, observed dependents/tests, weak graph components, and primary/planning
confidence. All path samples share the capability-advertised limit; component records are
separately bounded and retain exact totals and omissions.

The projection uses only analysis already requested by the caller. It survives summary,
change-summary, and NDJSON output, and is removed from baseline-ready and graph-only formats. Graph
components describe observed topology; they do not prove independent tasks or prescribe
delegation.

Opt-in blocks stay absent unless requested:

- `context`: `--context`, context limits, or `--focus`
- `directories`: `--by-dir`
- `baseline`: `--baseline`
- `graph`: `--graph`
- `impact`: `--impact`
- `review`: `--review`

Use `skip_serializing_if` for empty or absent blocks. Diff scope through `--since`, `--staged`, or
`--working` uses `git::changed_files` plus `DiffScope` and filters the primary file set before
analysis, so ordinary aggregates reflect only the changeset. Impact and diff-seeded context
deliberately retain scoped metrics while consulting full-tree topology; context also consults
cached full-tree file facts. A subpath target still limits which changed paths seed these modes.

## Baseline compatibility

New reports carry additive `analysis_profile` metadata for analyzer availability, diff scope,
health policy, duplication settings, and finding settings. A baseline must match that profile,
target scope, schema version, and effective token encoding. Omit metrics for disabled
analyzers and signals that are not comparable measurements, including configured test-file counts.

Finding comparison is complete only when both catalogs and finding profiles are compatible. Older
compatible reports without catalogs are aggregate-only. Reports predating analyzer-profile
metadata are rejected because their health semantics cannot be established.

## Agent task-query contracts

- `reposcout capabilities -f json` performs no scan and describes commands, formats, profiles,
  limits, language coverage, and machine interfaces.
- `--agent-summary` advertises its JSON format, default profile, projection strategy, total byte
  ceiling, and per-section entry limits through capabilities.
- `reposcout locate SYMBOL [PATH]` uses cached first-class declaration outlines with deterministic
  case-insensitive ranking or case-sensitive `--exact`, optional kind/language filters, and a hard
  100-result cap.
- Locate's cold path intentionally performs configured per-file analyzers, but not duplication or
  churn, to populate the ordinary scan cache. Do not create a second query-only parser, index, or
  cache profile.
- Capability tests compare advertised commands with Clap, symbol kinds with parser output, and
  language names with the canonical 31-format fixture matrix.
- `--error-format json` emits one structured stderr object for usage and runtime failures.
- Do not add an MCP dependency or parallel query implementation. Stable CLI JSON/NDJSON and shared
  query contracts are the automation surface; the roadmap keeps MCP out of scope.

## Renderer contracts

SARIF, NDJSON, DOT, and Mermaid are pure renderers over the same `ScanReport`.

- Without review, SARIF exposes duplicated code, high-complexity functions, and graph orphans.
  With review, it emits only review findings and maps deep states to SARIF baseline states.
- NDJSON records carry `kind`. They include summary metadata and optional context, then files,
  duplicate pairs, and review findings; full NDJSON places a requested graph in the summary record.
- DOT and Mermaid render the existing graph projection and never run external tools.
- Explain has focused table, JSON, Markdown, and NDJSON renderers and rejects SARIF and graph-only
  formats.
- Select formats with `-f` or infer them from the `-o` extension. Keep all renderers projections of
  shared model facts rather than analyzer implementations.
