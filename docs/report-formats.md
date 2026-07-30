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
reposcout -f markdown .
reposcout -f ndjson .
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

## Compact JSON

Full JSON includes every analyzed file, duplicate group, pair finding, and canonical finding. Use
`--summary` for an agent-sized scouting payload:

```sh
reposcout -f json --summary --profile agent .
```

Summary mode removes the heavy arrays while retaining:

- `summary` and `diagnostics`;
- bounded raw `work_scope` evidence;
- analysis/execution profile metadata;
- `summary.top_duplicates` and other top-N evidence; and
- explicitly requested `context`, `directories`, `graph`, `impact`, `baseline`, or `review`
  blocks.

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

Scan reports carry `schema_version: "1.0"`. The top-level contract is organized around:

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
