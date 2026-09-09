# Read explicit definitions

← [Documentation index](README.md)

Use `reposcout read` when a file and symbol or line are already known and the next decision needs
that definition's source. It returns complete supported definitions under one shared output budget.
It does not choose which function is relevant from a bare file path.

`read`, including `--outline`, is available on Unix platforms. The project's release targets are
Apple Silicon macOS and x86-64 Linux. Windows and other non-Unix builds reject the command before source I/O,
without a fallback reader. This restriction does not change repository inventory support.
Capabilities expose `source_query.available` for the current platform and
`source_query.platforms: ["unix"]` for the supported platform family.

A normal short read can still be enough, especially for a small file. Do not reread unchanged
source already available in the agent's context. There is no required scout, outline, or lookup
call before an explicit read.

## Select known definitions

```sh
# Read an explicitly named definition.
reposcout read . --symbol src/service.ts Service.start -f json

# Read the innermost declaration containing a known line.
reposcout read . --line src/service.ts 42 -f json

# Several known targets share one output budget.
reposcout read . \
  --symbol src/service.ts Service.start \
  --line src/client.ts 27 \
  --budget 4096 --max-output-bytes 65536 -f json
```

`--symbol` and `--line` each take two values and are repeatable. A query accepts at most 32 explicit
targets. CLI batching processes all `--symbol` pairs in their input order, followed by all `--line`
pairs in their input order; interleaving those flags does not interleave their results. `--outline`
is a separate mode. Budget admission and one-based target IDs follow this order. The shared query
API preserves the order of its supplied target vector.

File paths are relative to the directory `[PATH]`, which defaults to `.`. RepoScout resolves that
explicitly chosen directory once to a canonical anchor. Absolute file paths may use the canonical
anchor or the original target alias, but must remain beneath that target. Components below the
anchor are opened without following symlinks; legitimate aliases in the chosen root path do not
bypass that source-file boundary. File selectors must be UTF-8, contain no `..` component and use
at most 4,096 bytes. Symbol selectors accept at most 1,024 bytes. Symbol matching is case-sensitive:
qualified exact matches take precedence over simple-name exact matches; multiple matches within
the selected tier are ambiguous.

Line positions are one-based. Only containing parent declarations are removed when choosing an
innermost span. Equal spans with multiple identities and sibling declarations on the same line
remain ambiguous; the shorter
sibling does not win merely because it occupies fewer bytes. Attribute or decorator lines can
belong to that declaration, including supported standalone
GDScript annotations. A newline after the declaration can lie outside its source span; omitting
that trailing separator does not make the definition partial.

Returned source can include a statically established wrapper beyond the declaration's own span.
Selection and retrieval ranges are therefore separate facts. Duplicate or overlapping selections
are handled deterministically without repeating the same source merely because multiple targets
selected it.

The first implementation reads the current worktree. It does not read Git's index or a base
revision, map changed lines, search task descriptions, or trace calls. Those are separate planned
features. Ordinary `locate`, scouting and context output retain their existing body-free defaults.

## Inspect body-free declarations when useful

```sh
reposcout read . --outline src/service.ts -f json
reposcout read . --outline src/service.ts --outline src/client.ts -f json
```

`--outline` is an alternative to `--symbol` and `--line`, not a prerequisite for them. It returns
body-free declarations from explicitly named files with a shared cap of 100 declarations and
visible omissions. It does not treat the already capped context-outline projection as a complete
source of declaration facts.

## Understand the output budget

| Option | Default | Accepted range |
|---|---:|---:|
| `--budget` | 4,096 tokens | 256–65,536 tokens |
| `--max-output-bytes` | 65,536 bytes | 1,024–1,048,576 bytes |

Both limits apply to the complete rendered output, including metadata, candidates, omission
records, formatting and the final newline. Token counting uses the effective `o200k_base` or
`cl100k_base` encoding. Pretty JSON, when requested with `--pretty -f json`, also has to fit.

A request below either minimum is invalid. Its documented minimal error envelope is not required
to fit an impossible requested limit. Every successful response to a valid request fits both
limits; RepoScout never cuts JSON bytes or silently returns the beginning of a definition as complete.
A full definition that does not fit is omitted with an explicit reason. There is no partial-source
mode in this version. Ambiguity retains at most eight candidates rather than choosing the first
name silently.

This budget differs from a context plan's `selected_tokens`: the plan estimates the source cost of
selected files, while `read` limits its own actual rendered response. Neither limit is a requirement
to spend the entire allowance.

## Verify content before rereading

```sh
reposcout read . --symbol src/service.ts Service.start \
  --expect-hash src/service.ts '<SHA256>' -f json
```

Each successfully captured file has a SHA-256 content identity. Supply a previously observed hash when a later
read must still refer to that content. Only selected files may have expected hashes. A hash is 64 hexadecimal characters, normalized
to lowercase; conflicting expectations for the same file are invalid. The expectation applies to
every selection from that file. A mismatch is stale and returns no source for that file.
Range extraction and returned bytes always use the same captured content; old spans are never
silently applied to changed worktree bytes.

Each file is captured once per invocation. The batch is not an atomic snapshot across files or a
Git snapshot, and files can change afterward. A file's hash, spans and returned source always refer
to that same captured file content. A local analysis-cache hit does not mean the agent still has
the source in its context.

## Coverage and discovery boundaries

Source-input limits are separate from output limits: at most 8 MiB per file, 32 MiB across the
query and 32 files. Stricter configured limits also apply. Explicit targets retain the existing
ignore, exclusion, configuration, target and no-follow boundaries; a direct path is not permission
to bypass discovery policy. The command defaults to the `agent` profile; `--profile safe` retains
its stricter limits and ignores project configuration. Common configuration and exclusion options
still apply. Cache data remains outside the scanned repository.

An explicit `-o/--output` uses the existing symlink-safe atomic writer. The output cannot overwrite
a selected source file or the query root, and its exact path is excluded from input.

Precise retrieval supports the documented declaration kinds of existing first-class code
languages. Recognition of 36 inventory formats does not imply definition extraction for all of
them. Unsupported declarations, parse or extraction gaps, policy-ineligible files and output
omissions must not be interpreted as proof that the code is absent.

The precise retrieval matrix uses canonical declaration kinds:

| Language | Supported kinds |
|---|---|
| Rust | `enum`, `function`, `method`, `trait`, `type` |
| Python, JavaScript | `class`, `function`, `method` |
| TypeScript, TSX | `class`, `enum`, `function`, `interface`, `method`, `type` |
| Go | `function`, `method`, `type` |
| PHP | `class`, `enum`, `function`, `interface`, `method`, `trait` |
| GDScript | `class`, `constant`, `enum`, `method`, `property`, `signal` |
| Godot Shader | `function` |

Godot Scene, Resource and Project data and other generic inventory formats do not receive precise
source-definition support. A supported kind still needs a valid, safely retrievable range in the
particular file; the file's extraction state and result status qualify that fact.

## Interpret machine results

The report identifies itself with `kind: "source_query"` and `mode: "source"` or `"outline"`.
It keeps file facts, target outcomes and source content separate:

- `files` identifies selected files with `id`, `path`, optional `language`, captured `sha256`,
  extraction status and available declaration/source-definition counts.
- `results` associates each one-based input `target` with a `status`, optional file/definition/source
  references,
  and bounded candidates with total/omitted counts. The `selection` reason names exact qualified or
  simple-name matching, innermost-line selection, or a file outline.
- `sources` contains shared chunks with `id`, `file`, `span` and `content`. A result references its
  chunk instead of repeating the same source for each target. File and chunk IDs are stable
  references within the response, not offsets into the arrays.
- `requested_targets` and `omitted_targets` preserve target accounting when the output budget
  removes detailed responses. If the root path is omitted to fit, `root_omitted` says so.

A definition's `declaration_span` determines selection; optional `source_span` describes the
complete retrievable range, which can include a static wrapper. Spans use half-open byte offsets
and inclusive one-based line bounds. The optional `signature` is body-free metadata, not a
substitute for a complete `sources[].content`.

| Result status | Interpretation |
|---|---|
| `complete` | A complete selected source range was delivered |
| `outline` | Body-free declaration information was returned |
| `ambiguous` | Multiple identities match; inspect candidate totals and omissions |
| `not-found` | The selector did not resolve within the observed extraction |
| `stale` | The captured file hash differs from the supplied expectation; no source |
| `excluded`, `invalid-path`, `ignore-error` | Discovery or path policy could not admit the target |
| `unsupported`, `unavailable`, `parse-error` | The required extraction is unsupported, unavailable or affected by parsing errors |
| `unreadable`, `not-regular-file` | The target could not be read as eligible regular text |
| `oversized`, `input-budget-exceeded`, `deadline-exceeded` | A source-input resource limit prevented reading |
| `budget-omitted` | The output budget omitted the requested delivery |

File extraction states (`available`, `parse-errors`, `unsupported`, `unavailable`) are separate
from these per-target results. A successful command can contain unresolved, stale or omitted
targets; inspect the result statuses and omission totals rather than relying only on exit code. A compact output or zero delivered chunks does not by
itself prove complete extraction or absence of a definition.

## Formats and agent routing

The command supports JSON, NDJSON, table and Markdown. Captured stdout defaults to JSON; an
interactive terminal defaults to table. Each format describes the same query facts and respects
the selected total budget. NDJSON emits one compact `source_query` record followed by a newline.
JSON and NDJSON preserve source bytes after JSON string decoding. Table and Markdown present
human-readable metadata and complete source ranges: newlines and tabs remain literal, while
other control characters, including carriage returns, are visibly escaped. Use JSON or NDJSON when
the exact decoded source content is needed.

Existing structured CLI errors remain available through `--error-format json`. Invalid options or
roots, unrecoverable source-loading or analysis failures, token-counter initialization or
serialization failures, and an unavoidable status envelope that cannot fit produce an error
rather than a successful partial document. On non-Unix platforms the command returns
`read is available only on Unix platforms` before reading source.

Choose the entry point already available:

- Known file and symbol or line: request the definition directly if its source is needed.
- Need a file's declaration surface: request its body-free outline.
- Unknown symbol location: use existing `locate` or ordinary text search, then read only the
  selected definition when necessary.
- Need a bounded multi-file reading plan: use context planning and inspect its evidence before
  reading additional source.

RepoScout executes no model, compiler or test command for retrieval. Debug logs retain their
source-free contract. Successful retrieval proves the selected syntax was returned completely;
it does not prove that the definition is relevant or sufficient for the whole task.
