# Reports and machine formats

← [Documentation index](README.md)

RepoScout can produce compact human summaries, stable machine reports, code-scanning findings,
streaming records, and graph-only exports from the same analysis facts.

## Formats

| Format | Best for |
|---|---|
| `table` | Interactive terminal use |
| `json` | Complete machine consumption and baselines |
| `markdown` | PRs, issues, and saved reports |
| `sarif` | Code scanning and CI findings |
| `ndjson` | Streaming/log pipelines |
| `dot` | Graphviz-compatible graph exports |
| `mermaid` | Markdown/GitHub graph diagrams |

Choose explicitly:

```sh
reposcout -f json .
reposcout -f json --pretty .
reposcout -f markdown .
reposcout -f ndjson .
```

JSON is compact by default for machine and agent consumption. Add `--pretty` only when a human
needs indented JSON; the flag rejects non-JSON output instead of silently doing nothing.

For ordinary agent orientation, prefer the purpose-built bounded projection:

```sh
reposcout --agent-summary .
```

When only part of a report answers the question, project it before passing stdout onward. Use
`jq -c` to keep the selected result compact; bare `jq` pretty-prints again:

```sh
reposcout -f json --summary --profile agent . \
  | jq -c '{diagnostics, assessment: .summary.assessment, work_scope}'
```

Or infer from a known output extension:

```sh
reposcout -o report.md .
reposcout -o report.sarif .
reposcout -o report.jsonl .
reposcout -o architecture.dot --graph .
reposcout -o architecture.mmd --graph .
```

The exact output path is excluded from the scan so a report cannot feed back into itself.

## Agent-summary projection

`--agent-summary` emits one JSON document with `report_kind: "agent-summary"`. It defaults to the
`agent` execution profile unless `--profile` is explicit and never changes the blank invocation or
ordinary JSON/NDJSON contracts.

The projection retains compact interpretation metadata, coverage without path samples, source and
recognized-inventory totals, assessment, available aggregate complexity/duplication/symbol/test
signals, and at most three entries from each health ranking. Disabled analyzer signals are omitted
instead of appearing as clean zeroes. When context was requested, it separately reports:

- the original token/file budget and plan-level omissions;
- seed, graph-covered-seed, dependent, and matching-test totals needed to qualify an empty result;
- up to five files backed by focus, change, matching-test, or direct graph evidence;
- up to three lower-priority `expand_if_needed` candidates;
- up to three outline-only oversized seeds; and
- up to three unmatched focus paths.

Every bounded list records `available`, `shown`, and projection-local `omitted`; context tiers also
record the corresponding exact token totals. Those counters do not replace scan coverage or
context budget omissions. Analyzer-specific Type-2/churn partiality appears only when that analyzer
ran, and graph diagnostics appear only when a graph consumer ran. `projection.entries_omitted`
sums details removed from this view, while `projection.byte_limit_reached` says the byte ceiling
constrained rendering. Bytes are never cut mid-document; if even the required envelope cannot fit,
rendering fails instead of emitting invalid JSON.

The fixed compact contract rejects `--pretty` and detailed directory, baseline, graph, impact,
review, source-snippet, or duplicate-pair output. Use ordinary `--summary` plus a targeted `jq -c`
projection when one of those explicit result blocks is the actual question.

## Compact JSON

Full JSON includes every analyzed file, duplicate group, pair finding, and canonical finding. Use
`--summary` when a smaller aggregate report must retain explicitly requested detail blocks:

```sh
reposcout -f json --summary --profile agent .
```

Summary mode removes the heavy arrays while retaining:

- `summary` and `diagnostics`;
- bounded raw `work_scope` evidence;
- analysis/execution profile metadata;
- redundancy-filtered `summary.top_duplicates`, optional
  `summary.top_production_duplicates`, and other top-N evidence; and
- explicitly requested `context`, `directories`, `graph`, `impact`, `baseline`, or `review`
  blocks.

For requested context plans, summary mode retains selected paths, scores, reasons, structured
evidence, budget/omission totals, and aggregate outline counts. It removes per-file declaration
objects and sets `context.outline_details_omitted: true` when at least one such object was removed;
omit `--summary` when signatures are needed. The underlying planning analysis is unchanged.

Summary JSON remains valid aggregate baseline input.

Use `--baseline-ready` when the artifact exists specifically for later comparison. It stays
compact but retains the complete `finding_catalog`, enabling finding-level comparison. It removes
`work_scope` because transient reading/impact facts do not participate in baseline compatibility.

## Change-summary projection

For one Git diff scope, `--change-summary` emits a separate bounded contract instead of a shortened
ordinary scan report:

```sh
reposcout --working --change-summary -f json .
```

The compact JSON/NDJSON record identifies itself with `report_kind: "change-summary"` and retains
only report identity, interpretation metadata, primary diagnostics, bounded `work_scope`, and
`change_summary`. It omits the ordinary `summary`, `files`, `duplicates`, `finding_catalog`, raw
`context`, and raw `impact` blocks. The projection includes:

- all changed-file counts and bounded path details;
- merged reading-order roles, known impact, and convention-matched tests, each with explicit
  totals or omitted-detail counters;
- separate observed-scope, repository-discovery, and test-mapping confidence;
- relevant versus outside-known-scope graph gaps with explicit omitted counts; and
- evidence-backed validation categories that were suggested but not executed.

JSON is deliberately compact rather than pretty-printed. Table and Markdown render the same
decision data for humans. SARIF remains a findings format, while DOT and Mermaid remain graph-only
formats, so those three reject `--change-summary` as a structured usage error.

Stable executive reason codes are `no-graph-eligible-changes`, `no-graph-covered-changes`,
`changed-graph-coverage-incomplete`, `relevant-graph-gaps`, `repository-graph-gaps`,
`scan-truncated`, `test-mapping-heuristic`, and `no-matching-tests`. Gap scopes are `changed`,
`known-impact`, `selected-context`, and `outside-known-scope`. Validation kinds are `mapped-test`,
`project-configuration`, `inspect-non-graph-change`, and `specialist-review`; their text is
guidance, not evidence that validation ran.

## Stable JSON contract

Scan reports carry `schema_version: "2.0"`. The top-level contract is organized around:

| Block | Contents |
|---|---|
| `root`, `target`, `generated_at`, `encoding` | Report identity |
| `analysis_profile` | Enabled analyzers, diff scope, health, duplication, finding, and resource policy |
| `execution` | Profile, config provenance, timings, cache facts, and safety limits |
| `diagnostics` | Discovery coverage, bounded unsupported-path examples, omitted-count completeness, and partial Type-2 state |
| `summary` | Repository totals, language rollups, rankings, risks, tests, and assessment |
| `work_scope` | Versioned raw inventory, seed, context-budget, impact, graph-component, and confidence facts |
| `files` | Per-file metrics and facts; omitted by compact modes |
| `duplicates` | Exact/near groups, precise pairs, and coverage; omitted by compact modes |
| `finding_catalog` | Complete versioned complexity, marker, duplication, and risk findings |

In agent-summary context output, inspect `outline_only` before `direct_evidence` for oversized
explicit seeds. Without a focus or change seed, `direct_evidence` is intentionally empty and
`expand_if_needed` is only a bounded orientation shortlist.

Optional blocks appear only when requested:

| Block | Enabled by |
|---|---|
| `context` | `--context`, `--focus`, or context budget flags |
| `directories` | `--by-dir` |
| `baseline` | `--baseline` |
| `graph` | `--graph` or graph-focus flags |
| `impact` | `--impact` with a diff scope |
| `change_summary` | `--change-summary` with exactly one diff scope |
| `review` | `--review` with a diff scope |

New fields are additive and use deserialization defaults. Breaking JSON changes require a schema
version bump.

### Quality projections

Risk entries in `summary.top_risks` and detailed `explain` output identify their
`algorithm_version` and retain the raw SLOC, cyclomatic, and churn inputs. The analysis finding
profile carries the same version so baselines do not compare scores produced by different
algorithms.

Detailed `explain` output always reports repository source inventory independently of test-runner
availability. Its configured test-file count is omitted when no supported runner is established;
the file-level testing block then reports `unavailable` instead of a synthetic zero or an inferred
source-to-test match.

`summary.duplication` and the top-level `duplicates` block describe the configured health corpus.
Compact `summary.top_duplicates` removes nested/substantially overlapping rankings, while
`summary.top_production_duplicates` additionally omits test-only and Rust-inline-test-only
families. A retained instance must contain at least `min_dup_lines` contiguous non-test lines, so
whitespace between adjacent inline tests cannot make a test-only family production-relevant. An
empty production projection is omitted from serialized output; it is not evidence that
duplication analysis was disabled.

`summary.assessment.production_duplication` is the explicit production-source coverage record. It
contains `corpus`, `duplicated_lines`, `analyzed_lines`, `duplicated_pct`, and `complete`, and is
absent only when duplication analysis did not run. The same record appears in `work_scope` strategy
`2`. When `complete` is false, table and Markdown render it as partial. It is a lower bound only
when Type-2 work was the sole gap; source discovery/read omissions can also change the denominator.

The Rust contract is defined in [`src/model.rs`](../src/model.rs). The most reliable way to inspect
real values is to emit a report from the installed binary:

```sh
reposcout -f json --baseline-ready tests/fixtures/sample/
```

## Analysis compatibility

Baselines must match:

- target scope and effective token encoding;
- analyzer availability;
- resolved diff base tree;
- health-file policy;
- duplication thresholds/mode/format scope; and
- finding profile; and
- effective file, byte, Git-blob, file-count, and scan-time limits.

Reports without modern analysis-profile metadata are rejected because their health semantics
cannot be established. Compatible reports without a finding catalog retain aggregate-only
comparison.

Finding comparison uses four states:

- `new`;
- `resolved`;
- `worsened`; and
- `improved`.

Only new and worsened findings count as regressions.

## SARIF

SARIF output is a SARIF 2.1.0 document:

```sh
reposcout -f sarif .
reposcout -o report.sarif --since main --review=deep .
```

Results include:

- precise duplicate pairs with related locations;
- callables above `--max-complexity`; and
- graph orphan candidates when graph analysis is enabled.

With review enabled, SARIF contains only the review result set. Deep comparison projects states
through SARIF `baselineState`.

## NDJSON

```sh
reposcout -f ndjson . > report.ndjson
```

Records are emitted in this order:

1. one aggregate `summary` record;
2. one `file` record per analyzed file; and
3. one `finding` record per duplicate pair.

The summary record also carries identity/profile metadata, diagnostics, and requested optional
blocks. Review findings use `kind: "review_finding"`. The first record carries `work_scope`, and
`--summary` emits only that record.

`reposcout explain FILE -f ndjson` emits one contextual record. Symbol lookup emits a query header
followed by one record per match. `--change-summary -f ndjson` emits one bounded record carrying
`report_kind: "change-summary"`.

## DOT and Mermaid

```sh
reposcout --graph-focus src/service.ts -f dot .
reposcout --graph-focus src/service.ts -f mermaid .
```

Both formats render the same deterministic selected graph. RepoScout invokes neither Graphviz nor
Mermaid; it writes plain text for downstream tools.

## Structured errors and exit codes

Use JSON errors in automation:

```sh
reposcout --error-format json --graph-depth 65 .
```

RepoScout writes one JSON object to stderr.

| Exit code | Meaning |
|---:|---|
| `0` | Success, including a broken downstream stdout pipe |
| `1` | Configuration, I/O, runtime, or post-parse validation error |
| `2` | Parser/change-summary usage error, requested gate, or regression condition |

See [CLI reference](cli-reference.md#ci-gates) for metric, review, and baseline gates.
