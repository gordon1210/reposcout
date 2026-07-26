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
- analysis/execution profile metadata;
- `summary.top_duplicates` and other top-N evidence; and
- explicitly requested `context`, `directories`, `graph`, `impact`, `baseline`, or `review`
  blocks.

Summary JSON remains valid aggregate baseline input.

Use `--baseline-ready` when the artifact exists specifically for later comparison. It stays
compact but retains the complete `finding_catalog`, enabling finding-level comparison.

## Stable JSON contract

Scan reports carry `schema_version: "1.0"`. The top-level contract is organized around:

| Block | Contents |
|---|---|
| `root`, `target`, `generated_at`, `encoding` | Report identity |
| `analysis_profile` | Enabled analyzers, diff scope, health, duplication, finding, and resource policy |
| `execution` | Profile, config provenance, timings, cache facts, and safety limits |
| `diagnostics` | Discovery coverage, omitted-count completeness, and partial Type-2 state |
| `summary` | Repository totals, language rollups, rankings, risks, tests, and assessment |
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
blocks. Review findings use `kind: "review_finding"`. `--summary` emits only the first record.

`reposcout explain FILE -f ndjson` emits one contextual record. Symbol lookup emits a query header
followed by one record per match.

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
| `1` | Usage, configuration, I/O, or runtime error |
| `2` | A requested gate or regression condition was met |

See [CLI reference](cli-reference.md#ci-gates) for metric, review, and baseline gates.
