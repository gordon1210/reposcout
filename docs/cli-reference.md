# CLI reference

← [Documentation index](README.md)

```text
reposcout [OPTIONS] [PATH]
reposcout <COMMAND> [OPTIONS]
```

`PATH` defaults to the current directory and may identify a repository, a subdirectory, or one
file.

> Because focused commands accept their own options, global flags go after the command:
> `reposcout tokens --encoding cl100k_base src/`.

## Commands

| Command | Purpose |
|---|---|
| `reposcout [PATH]` | Run the configured full scan |
| `reposcout tokens [PATH]` | Count tokens only |
| `reposcout complexity [PATH]` | Analyze complexity only |
| `reposcout dup [PATH]` | Detect duplication only |
| `reposcout churn [PATH]` | Analyze Git churn and hotspots only |
| `reposcout metrics [PATH]` | Run tokens, markers, and imports |
| `reposcout explain FILE` | Explain one file in its full repository context |
| `reposcout locate SYMBOL [PATH]` | Find declarations across first-class languages |
| `reposcout capabilities` | Describe the installed machine contract without scanning |
| `reposcout config [PATH]` | Inspect layered configuration and effective values |
| `reposcout cache clear [PATH]` | Clear one repository's analysis and Git-history caches |
| `reposcout cache clear --all` | Clear every RepoScout cache |
| `reposcout update` | Install the latest stable release for an installer-managed copy |
| `reposcout daemon [PATH]` | Watch a target and serve live results |

`locate` ranks case-insensitive matches by qualified exact, simple exact, prefix, and substring
quality. Add `--exact`, `--kind`, `--language`, or `--limit` to narrow the result. It supports
table, JSON, Markdown, and NDJSON output and reuses the ordinary scan cache.

## Core options

| Flag | Description | Default |
|---|---|---|
| `-f, --format <FORMAT>` | `table`, `json`, `markdown`, `sarif`, `ndjson`, `dot`, or `mermaid` | table on TTY, otherwise JSON |
| `-o, --output <FILE>` | Write to a file and infer known formats from its extension | stdout |
| `--profile <full\|agent\|safe>` | Select the execution profile | `full` |
| `--no-project-config` | Ignore repository-owned configuration | off |
| `--error-format <text\|json>` | Render failures as text or one JSON stderr object | `text` |
| `--pretty` | Pretty-print JSON instead of the compact default | off |
| `--debug-log <FILE>` | Write immediately flushed NDJSON diagnostics | off |
| `--only <LIST>` | Run only named analyzers | all |
| `--exclude <GLOB>` | Extend configured discovery exclusions; repeatable | — |
| `--include-lockfiles` | Include recognized dependency lockfiles | off |
| `--encoding <NAME>` | `o200k_base` or `cl100k_base` | `o200k_base` |
| `--hidden` | Include hidden files | off |
| `--no-ignore` | Ignore `.gitignore`; `.reposcoutignore` still applies | off |
| `-j, --jobs <N>` | Worker threads for file analysis and duplication tokenization | CPU count |
| `--max-file-bytes <BYTES>` | Largest recognized worktree file accepted | 32 MiB |
| `--max-total-bytes <BYTES>` | Aggregate recognized bytes accepted per discovery pass | 512 MiB |
| `--max-files <N>` | Files observed before discovery stops | `100000` |
| `--max-git-blob-bytes <BYTES>` | Largest Git blob accepted by deep review | 32 MiB |
| `--max-scan-seconds <SECONDS>` | Cooperative wall-clock budget | `1800` |
| `--no-cache` | Disable the incremental cache for this invocation | off |
| `--top <N>` | Length of top-N projections | `10` |
| `-q, --quiet` | Hide progress feedback | off |

## Health and duplication

| Flag | Description | Default |
|---|---|---|
| `--max-complexity <N>` | Report callables whose cyclomatic complexity exceeds `N` | `20` |
| `--health-scope <source\|all>` | Choose the starting corpus for health analysis | `source` |
| `--health-include <FORMAT>` | Add a non-source format to health analysis; repeatable | — |
| `--health-exclude <GLOB>` | Remove repository-relative paths from health analysis while retaining inventory; repeatable | — |
| `--dup-mode <strict\|mild\|weak>` | Keep all trivia, ignore whitespace, or also ignore comments | `mild` |
| `--dup-format-scope <exact\|compatible\|all>` | Choose cross-format candidate pools | `exact` |
| `--dup-snippets` | Include bounded source snippets in duplicate findings | off |
| `--dup-details` | Expand precise duplicate pairs in human reports | off |

`compatible` combines JavaScript, TypeScript, and TSX; `exact` isolates every detected format.
Duplication token, line, and similarity thresholds are configurable in
[`reposcout.toml`](configuration.md#example-reposcouttoml).
Health selection always applies scope first, format includes second, and path excludes last, so an
exclude wins over an include. Ordinary `--exclude` removes a path from the entire scan instead.

## Context, structure, and graph

| Flag | Description | Default |
|---|---|---|
| `--summary` | Remove heavy arrays from JSON while retaining requested query blocks | off |
| `--context` | Add a deterministic, token-budgeted reading plan | off |
| `--no-context` | Disable a context plan enabled by configuration | off |
| `--context-budget <TOKENS>` | Hard aggregate selected-token budget | `32000` |
| `--context-max-files <N>` | Hard selected-file cap | `25` |
| `--focus <PATH>` | Prioritize a path and its test/graph neighborhood; repeatable | — |
| `--by-dir[=DEPTH]` | Add a directory rollup at the requested depth | off |
| `--graph` | Build the full supported dependency/type graph | off |
| `--graph-focus <PATH>` | Select a bounded graph neighborhood; repeatable | — |
| `--graph-depth <0..64>` | Maximum focus traversal hops | `1` |
| `--graph-direction <dependencies\|dependents\|both>` | Choose graph traversal direction | `both` |

DOT and Mermaid formats render only the requested graph projection and do not invoke an external
graph tool.

## Change, review, and baselines

| Flag | Description | Default |
|---|---|---|
| `--since <REF>` | Scan files changed since a Git ref | — |
| `--staged` | Scan staged changes | off |
| `--working` | Scan uncommitted working-tree changes | off |
| `--change-summary` | Emit a bounded change decision and imply context/impact | off |
| `--impact` | Report direct and transitive internal dependents for a diff scope | off |
| `--review[=lines\|deep]` | Filter current findings or compare both Git snapshots | off |
| `--fail-on-review` | Exit `2` for actionable review findings | off |
| `--baseline-ready` | Emit compact JSON with the complete finding catalog | off |
| `--baseline <FILE>` | Compare against a compatible JSON report | — |
| `--fail-on-regression` | Exit `2` when the baseline comparison regresses | off |
| `--fail-on <EXPR>` | Exit `2` when any metric expression is true | — |

Only one of `--since`, `--staged`, and `--working` may define the diff scope.
`--change-summary` requires one of them. It defaults to the `agent` profile unless `--profile
full` or `--profile safe` is explicit, and supports table, JSON, Markdown, and NDJSON only.
Capability discovery reports its fixed path, gap, and validation limits.
It also reports the additive work-scope strategy version, aggregate path bound, and component
bound. Work-scope facts appear in ordinary JSON/NDJSON and human reports without enabling
additional analysis; `--baseline-ready` and graph-only formats omit them.

## Common examples

### Scout and query

```sh
# Human summary
reposcout .

# Compact agent payload
reposcout -f json --summary --profile agent src/

# Guardrailed scan of an untrusted checkout
reposcout -f json --summary --profile safe .

# Discover the installed contract
reposcout capabilities -f json

# Locate one exact declaration
reposcout locate HttpClient . --exact --kind class -f json

# Explain one file in repository context
reposcout explain src/service.ts
```

### Plan and inspect change

```sh
# Reading plan under hard limits
reposcout -f json --summary --context-budget 24000 --context-max-files 15 .

# Focus the plan on one service
reposcout -f json --summary --focus src/service.ts .

# Keep metrics change-scoped while consulting full-tree planning/topology
reposcout --working --change-summary -f json .

# Request the detailed context and impact blocks
reposcout -f json --working --context --impact --profile agent .

# Deep two-snapshot review
reposcout -f sarif --since main --review=deep --fail-on-review .
```

### Duplication and graph

```sh
# Ignore comment-only differences and show precise pairs
reposcout dup --dup-mode weak --dup-details src/

# Include authored styles in health analysis
reposcout --health-include css --health-include scss .

# Keep third-party source in inventory but out of health signals
reposcout --health-exclude 'packages/ui/src/components/**' .

# Two-hop reverse dependency neighborhood
reposcout --graph-focus src/service.ts --graph-direction dependents \
  --graph-depth 2 -f mermaid -o service-radius.mmd .

# Complete mixed-language graph
reposcout --graph -o architecture.dot .
```

### Reports and baselines

```sh
# Infer Markdown, SARIF, or Mermaid from the extension
reposcout -o STATUS.md src/
reposcout -o report.sarif src/

# Stream records
reposcout -f ndjson src/ > report.ndjson

# Save and later compare a finding-complete baseline
reposcout --baseline-ready -o baseline.json src/
reposcout --baseline baseline.json --fail-on-regression src/
```

## CI gates

`--fail-on` accepts comma-separated `key OP number` conditions. If any condition is true,
RepoScout exits with code `2`; ordinary errors exit `1`.

```sh
reposcout --fail-on "max-cyclomatic>30,duplicated-pct>5,min-mi<50" src/
```

Supported keys:

- `max-cyclomatic`, `avg-cyclomatic`
- `max-cognitive`, `avg-cognitive`
- `min-mi` / `min-maintainability`
- `avg-mi` / `avg-maintainability`
- `duplicated-pct`, `tokens`, `files`, `sloc`

Supported operators are `>`, `<`, `>=`, `<=`, and `==`. Conditions requiring a disabled analyzer
are rejected instead of evaluating an invented zero. Complexity gates operate on callables, not
whole-file totals.

For change-over-time gating, use `--baseline-ready` with
`--baseline <FILE> --fail-on-regression`. For changed-code gating, combine a diff scope with
`--review --fail-on-review`.

## Debugging slow or crashing runs

Pass `--debug-log <FILE>` to any command:

```sh
reposcout --debug-log /tmp/reposcout-debug.jsonl .
```

The path must not exist. It is excluded exactly from the scan and daemon watcher. Every NDJSON
event is flushed immediately and contains timing/thread metadata plus event-specific data.

Useful records include:

- `stage_start` / `stage_end` for broad phases;
- `discovery_progress` for large walks;
- `file_start` / `file_end` for per-worker analysis;
- two-second `heartbeat` records during quiet work;
- rate-limited `type2_progress` counters and safety-limit state;
- `render_*` / `output_*` to separate analysis from serialization; and
- `panic` with location and backtrace before the normal panic hook.

Logs contain paths, arguments, configuration, timings, and backtraces, but never source contents.
Review them before sharing.

See [Configuration and caching](configuration.md) for saved defaults and cache maintenance.

Resource limits are clamped to non-disableable absolute ceilings. A scan that reaches one remains
successful but marks its diagnostics as truncated and reports the omitted files/bytes or expired
deadline; automation should check those fields before treating absent findings as evidence. When
traversal must stop at a safety bound, `files_omitted_by_limit` is the number already proven to be
omitted and `files_omitted_count_incomplete: true` states that the exact remaining count was
deliberately not computed.
