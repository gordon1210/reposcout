# Agent report and mode contracts

This is a normative extension of the root [`AGENTS.md`](../../AGENTS.md). Read it completely before
changing report shapes, summary/change/review/baseline behavior, CLI task queries, or renderers.
The root instructions remain in force.

## Pinned PR review preparation

`review-context` is a separate Unix task query over two pinned Git commits/trees. Keep direct
comparison and explicit unique-merge-base comparison distinguishable. Inventory must account for
deleted, renamed, excluded and unsupported paths; unknown analysis or costs must not become zero
evidence. Preserve per-side snapshot identity, mapping ambiguity, graph coverage and independent
capture, context-selection and rendered-output omissions. Source and diff bodies remain opt-in.
Reuse shared snapshot capture, file analysis/cache, change mapping and graph resolvers; renderers
are pure projections. See [review context](../review-context.md) for the command contract.
Zero-length hunk sides can retain existing declaration counterparts through unchanged headers.
Counterpart work gaps remain separate from raw line mapping; incomplete supplying-side mappings
must keep counterpart coverage partial even when every supplied declaration was processed.
Budget projection reconsiders omitted identities against the actual format/tokenizer and retains compact
file identities ahead of declaration/range details; per-side output omissions must not downgrade
analysis availability or be confused with absent declarations.
`--brief` is an opt-in Table/Markdown projection of the same facts; without `--budget` it renders
in full up to a 4,096-token ceiling, and an explicit `--budget` always wins. It omits per-file
identities, folds identifier renames repeated across hunks and moved lines into summaries, shows
hunks explained by other repeated rewrites compactly (changed lines only), flags old
spellings left in head, lists only incoming distance-1 impact and nonzero gaps of the changed
files, counts transitive dependents, and adds head-snapshot doc mentions and text-matched usage
lines of direct importers without resolved references, all held outside the JSON contract
(`ReviewBrief` is serde-skipped). Only renames proven pure at every site fold without showing
their lines, and only renames of plain bindings: both tokens are plain identifier nodes of the
complete base and head files' syntax trees in an allowlisted grammar (TS/TSX/JS, Python, Rust, Go),
never member, key, field, attribute or argument-name positions; the old name is bound in the
site's base file and the new name in its head file by declarations or by imports from in-tree
modules (relative TS/JS specifiers, relative or captured Python modules, Rust `crate`/`self`/`super`
paths), never by external imports, ambient, `extern` or ABI items (`#[no_mangle]`,
`#[export_name]`, `extern "C" fn`), Rust `macro_rules!` names or Python keyword-only parameters,
and the file's renames are one-to-one; the new name occurs in no base-tree
blob and the old name in no head-tree blob (bounded scan; over the bound nothing is pure); and
spelling changes with language meaning (Go export case, Python underscores and dunders, test
affixes in test files) never qualify. A moved declaration folds only when the target file's other
top-level declarations of names it uses match the source file byte for byte and its enclosing
container headers are byte-identical on both sides.
Keyword, value-like name, number, string, operator, punctuation, insertion, deletion and
multi-token edits and unproven renames always show their changed lines. Any fold needs equal
line counts per block and byte equality of each paired line after rewrites. Folding never hides
a reorder within one hunk, a reversed rename, an indentation change in an indentation-sensitive
language (such hunks render in full), or moved code other than whole declarations whose every
line pairs exactly after pure renames (never between a test and a non-test file); renames, mode
changes and added, deleted or modified files whose hunks moved always stay listed. Its projection
admits items in a fixed order under both budgets (entry headings, hunks with details after each
change's first hunk, every other non-test hunk before every other test-file hunk and compact
hunks after all others, examples, then soft evidence; a bounded greedy pass afterwards admits no
entry or hunk) and keeps soft evidence within max(400, diff tokens / 2). Mandatory summaries
collapse in three levels before a budget is rejected. Each finer level, and finally compact-hunk
sites, has a budget-independent start and takes over from it once it admits at least as many
entries and hunks as the coarser levels keep from just below that start, so a larger budget never
drops a shown entry or hunk and never coarsens the level. Omitted compact hunks are grouped by
rewrite combination (sites and fetch commands only once every other hunk is shown, otherwise
counts and one command). Other omitted hunks are listed per file for up to 40 files, then counted;
level 2 reduces both lists to one count line with a command each, level 3 to counts and one
full `git diff`. Commands use 12-character resolved commits (the merge base with
`--merge-base`), `--no-indent-heuristic` to match the internal hunks, and shell-quoted
`:(top,literal)path` pathspecs. Output is byte-identical for identical input. Existing flag
combinations and JSON output stay unchanged.

RepoScout never allocates agents or automatically partitions review work. Tokenization remains the
existing default/configuration/explicit CLI choice, without model-based selection.

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
- `reposcout read [PATH]` accepts repeatable `--symbol FILE SYMBOL`, `--line FILE LINE` and
  explicit whole-file `--file FILE` and line-excerpt `--range FILE START END`, or
  mutually exclusive repeatable `--outline FILE` and defaults to the `agent` profile. It reads
  explicit targets from `--snapshot worktree|index|REF` (default worktree); no preceding scout or
  outline/locate is required. Target-relative paths and absolute paths inside the
  target retain discovery, exclusion, configuration and no-follow policy. CLI target order is all
  symbol pairs, line pairs, file selectors, then range triples, preserving order within each group; target IDs
  are one-based and budget admission follows that order. The query API preserves vector order.
  Source and outline queries are Unix-only; `source_query.available` reflects the current build
  and `source_query.platforms` is `["unix"]`. Non-Unix calls fail before source I/O, without a
  fallback. Existing repository inventory support is unchanged.
- `reposcout changes [PATH]` requires one working/staged/since diff scope and defaults to the agent
  profile. Selection is body-free unless `--source` is explicit. Both source sides, pinned revision
  identity, mapping/capture gaps and output omissions survive bounded rendering. It uses the same
  Unix platform restriction and shared response budgets as explicit reads.
- `--changed-definitions` requires change-summary or review plus exactly one diff scope. It supports
  table/JSON/Markdown/NDJSON and rejects SARIF/DOT/Mermaid, agent-summary and baseline-ready.
  The body-free `definition_changes` block is bounded separately to 4,096 tokens and 16,384 bytes
  of its compact JSON representation. This is not a bound on the whole surrounding report;
  existing parent projection limits remain unchanged. Do not silently drop the requested block.
- Source-query budgets cover the complete rendered response in the selected format, including
  metadata and newline: 4,096 tokens / 65,536 bytes by default, with allowed ranges 256–65,536
  tokens and 1,024–1,048,576 bytes. Below-minimum requests are invalid and return the documented
  minimum error envelope, not a falsely budget-compliant success.
- Source queries return complete definitions, explicitly selected files/ranges, or explicit
  omissions, never implicitly clipped bodies. Ranges use inclusive one-based physical lines, exact
  captured bytes and no EOF clipping. An empty file has no selectable physical line. `selection: "range"` and additive `requested_range` identify the requested excerpt without a definition;
  shared source unions may cover other explicitly selected overlapping targets. Normal omissions
  retain requested bounds; the minimal status envelope may omit them with other selection metadata. File reads retain capture/hash checks and declaration-extraction
  metadata without requiring successful extraction or fabricating a definition. Recognized text
  formats are eligible; unknown-extension policy is unchanged. `plan --file` remains a declaration seed.
  Cap explicit read targets at 32, ambiguity candidates at eight and outline declarations at 100;
  admit at most 128 derived change targets while retaining exact total/omitted counts. Retain
  extraction/input coverage separately from output omissions. `--expect-hash FILE SHA256` only
  applies to selected files; stale files return no source.
- Source-query renderers project shared facts and perform no analysis or I/O. JSON, NDJSON, table
  and Markdown all respect the final output budget; pretty formatting and newlines count.
  NDJSON emits one compact `source_query` or `change_query` record according to the query. JSON strings preserve decoded source; human
  output retains newlines/tabs and visibly escapes other control characters, including CR.
  Ordinary locate, context and agent-summary remain body-free. Debug logs never contain source.
- Capability tests compare advertised commands with Clap, symbol kinds with parser output, and
  language names with the canonical 36-format fixture matrix.
- `--error-format json` emits one structured stderr object for usage and runtime failures.
- Do not add an MCP dependency or parallel query implementation. Stable CLI JSON/NDJSON and shared
  query contracts are the automation surface; the roadmap keeps MCP out of scope.

## Lexical, definition-plan and consumer query contracts

- `find` uses deterministic lexical field evidence and independent search-coverage and output
  omission counters. It returns body-free candidates with content-checked read targets; it never
  infers relevance from health rank or silently changes `locate` semantics.
- `plan` uses pure captured-fact selection with separate source-union and rendered-output budgets.
  Its optional shared source block must fit the complete combined response. Unresolved seeds,
  uncosted facts, missing environment and output projection remain separate coverage dimensions.
- `consumers` is worktree-only and returns bounded unique reachable symbols with shortest evidence.
  Seed identity and expected hashes fail closed; read handoffs use qualified symbols and hashes,
  not a line heuristic that might select a nested declaration. Call/reference topology never
  changes import impact or type-reach meanings. Truncated fact sets cannot establish safe bindings.
  Module-scope `(module)` sources are not consumer hits; review-context still shows them as call
  evidence.
- All three preserve bounded table/JSON/Markdown/NDJSON formats, structured errors and pure
  renderers. Keep actual fields, limits and supported syntax synchronized with capabilities and
  [task-query documentation](../task-queries.md).

## External task diagnostics

`--task-diagnostics` is explicit runtime-only file/stdin input and implies context. Its format flag
requires input. SARIF/DOT/Mermaid output and `--no-context` are rejected; agent-summary remains
allowed under its existing output ceiling. No tool execution, project-configured input path,
raw-log persistence or health-finding conversion is permitted.

External coverage lives in `context.task_evidence`, never replacing top-level scan diagnostics.
Normal/safe input, record and detail limits are independent. Lower-bound omitted-record counts
must carry `omitted_records_exact: false`; compact detail omission is counted separately. Human
output uses compact counts, while agent-summary retains at most three gap-first details and exact
projection omissions. See [task diagnostics](../task-diagnostics.md).

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
