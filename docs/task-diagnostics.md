# External diagnostics as task evidence

← [Documentation index](README.md)

Import an existing compiler, linter or test diagnostic file when its locations can narrow the next
reading decision. RepoScout normalizes bounded input and uses resolved locations as seeds in the
existing context planner. It does not execute the producer, interpret arbitrary logs semantically,
persist logs or turn external diagnostics into health findings.

This implementation is covered by the [bounded native pilot](agent-evaluation.md). Its limits are not a
claim that full agent comparisons have passed.

## Supply an explicit input

```sh
reposcout --task-diagnostics results.sarif --task-diagnostics-format sarif \
  --summary -f json .

reposcout --task-diagnostics compiler.jsonl --task-diagnostics-format rustc-json \
  --summary -f json .

reposcout --task-diagnostics build.log --task-diagnostics-format text \
  --working --change-summary -f json .

cat compiler.jsonl | reposcout --task-diagnostics - --summary -f json .
```

`--task-diagnostics PATH|-` takes one explicit regular file or piped stdin and implies context.
`--task-diagnostics-format auto|sarif|rustc-json|text` requires the input and defaults to `auto`.
Input from an interactive terminal is rejected instead of waiting indefinitely. The flags conflict
with `--no-context`. Input paths are runtime arguments, never project-configuration entries.

Table, JSON, Markdown and NDJSON preserve diagnostic context. SARIF, DOT and Mermaid output are
rejected for this workflow rather than silently losing the requested evidence. Compact
`--agent-summary` is allowed and keeps its existing hard output ceiling.

## Formats and normalization

- **SARIF 2.1.0:** bounded physical locations become separate records. The parser retains producer,
  rule code, severity and one-based region positions; missing SARIF severity defaults to warning.
- **Cargo/rustc JSON:** compiler-message NDJSON contributes primary spans. Build artifacts and the
  compiler-rendered multiline message are not copied into task evidence.
- **Conservative text:** recognized location forms supply partial-confidence evidence. Unsupported
  severity remains unknown; the parser does not infer broad framework semantics.

Automatic format selection examines a bounded prefix once. JSON-like input that does not match a
supported structure is an error, not a text fallback. Explicit text mode is available when the
caller deliberately wants heuristic location extraction.

Messages become bounded single-line text; ANSI and controls are removed or escaped. IDs are
assigned deterministically after normalization, sorting and deduplication, and are report-local,
not stable cross-report fingerprints. Structured inputs carry high confidence; heuristic text
carries partial confidence.

SARIF URI paths are percent-decoded once, including relative URIs. Percent characters in literal
rustc/text paths remain literal. Resolution uses exact repository-relative, target-relative or
root-internal absolute identities from the existing inventory. It does not guess by basename or
suffix, perform another walk, follow symlinks or admit external URI schemes. Valid repository
paths outside a selected subpath are out of scope; missing or invalid paths remain unresolved.
Only resolved in-target locations become seeds. Existing diff-context planning can still select
related files from its already-defined full-tree planning universe.

## Input and coverage limits

| Bound | Normal | Safe |
|---|---:|---:|
| Input bytes | 8 MiB | 1 MiB |
| Normalized records | 1,000 | 250 |
| Serialized details | 100 | 50 |
| Message length | 512 Unicode scalar values | 512 |
| Tool/code length | 128 Unicode scalar values | 128 |

The reader may consume one extra byte to detect input truncation. Input truncation, record
truncation, parse errors and detail omissions are independent. When the unprocessed suffix is not
countable, `omitted_records_exact: false` means the recorded omitted count is a known lower bound;
it is not the total number of diagnostics that could exist. Detail omissions are counted exactly
within the retained normalized record set.

Useful mixed or bounded input can produce partial evidence. An explicitly selected structured
format with no usable records because of malformed input fails; an empty valid input retains an
explicit empty-evidence result. A successful command with unresolved or truncated input is not
proof that every diagnostic was understood.

## Seed the existing context planner

With diagnostic input, conditional context strategy 4 preserves this direct-intent order when other
conditions are equal: explicit focus, resolved errors, changed paths, warnings, then notes/info or
unknown severity. Repeated diagnoses add a capped boost of at most three records per file. No-input
context keeps strategy 3 and its established behavior.

Resolved diagnostic seeds survive an insufficient source budget as `outline_only` evidence, even
when no declaration headers are available; an empty symbol list means exactly that. Graph neighbors
reuse existing relation facts and provenance. Up to eight diagnostic IDs support each evidence
entry; transitive evidence traces a shortest source path and does not claim to enumerate every
contributing diagnosis. Filenames alone do not establish test-to-source or source-to-test mappings.

A resolved position can be explicitly passed to the existing source reader or definition planner:

```sh
reposcout read . --line src/billing.rs 42 -f json
reposcout plan . --line src/billing.rs 42 --context-budget 4000 -f json
```

A diagnostic log is not a source snapshot. Those commands capture current content unless a
snapshot is explicitly selected; do not treat an old log position as proof that the file is
unchanged. Use observed source hashes when preserving identity between later query steps. An
unresolvable position remains file-level evidence with a gap, not a guessed definition.

## Interpret the report

`context.task_evidence` reports external-input format/status, bytes and record counts, deduplication,
resolved/unresolved/out-of-scope locations, parse errors, truncation and bounded details. Existing
top-level `diagnostics` continues to mean repository scan coverage. Health scores, finding catalogs,
regression gates and ordinary scans without input retain their meanings.

Human summaries show compact counts and truncation, not individual diagnostic messages. Full
JSON/NDJSON retain bounded normalized details. Agent-summary retains compact task counts and up to
three details selected to expose gaps first, with its own shown/omitted accounting and further
byte-budget pruning. It is not an exhaustive copy of the imported log.

Normalize before sending a large log to an agent when that fits the task. If the agent already read
the log, count those tokens as spent; later normalization cannot retroactively save them. Additional
reads, failed parses, retries and missing-context work belong in the [actual evaluation](../scripts/agent-eval/README.md).
