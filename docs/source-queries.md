# Source and changed-definition queries

← [Documentation index](README.md)

Use `reposcout read` when a file and symbol or line are already known and the next decision needs
that definition's source. It returns complete supported definitions under one shared output budget.
Use explicit `--range FILE START END` for a known small module-level span, or `--file FILE`
when complete module imports, registrations or other file context are needed. It does not choose which function is relevant from a bare file path. Use `reposcout changes` when
the entry point is a working-tree, staged or revision-based diff instead of a known symbol.

`read`, including `--outline` and snapshot selection, and `changes` are available on Unix platforms. The project's release targets are
Apple Silicon macOS and x86-64 Linux. Windows and other non-Unix builds reject the command before source I/O,
without a fallback reader. This restriction does not change repository inventory support.
Capabilities expose `source_query.available` for the current platform and
`source_query.platforms: ["unix"]` for the supported platform family.
`change_query.available` advertises direct changed-definition availability and its capability
block lists the scopes, work limits and embedded-report limits.

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
targets. CLI batching processes all `--symbol` pairs, then `--line` pairs, `--file` selectors and
`--range` triples, preserving input order within each group; interleaving flags does not interleave results. `--outline`
is a separate mode. Budget admission and one-based target IDs follow this order. The shared query
API preserves the order of its supplied target vector.

File paths are relative to the directory `[PATH]`, which defaults to `.`. RepoScout resolves that
explicitly chosen directory once to a canonical anchor. Absolute file paths may use the canonical
anchor or the original target alias, but must remain beneath that target. Worktree components below the
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

## Read explicitly needed file context

```sh
reposcout read . --file src/routes.py --file src/handler.py \
  --expect-hash src/routes.py '<SHA256>' --budget 4096 -f json
```

`--file` is repeatable and can share a batch with symbol or line selectors. It returns the complete
captured text, including imports, registration tables, comments and trailing newlines, or an
explicit budget omission. It never silently clips the file. Overlapping definition and file
selections share source chunks. An empty file is a complete empty chunk with a content hash.

Prefer definition reads when their smaller source already answers the question. Request a file
only when its surrounding context matters, and batch known paths under one budget. There is no
automatic expansion to imported files or model-dependent tokenizer selection. `plan --file`
continues to seed declarations; it does not acquire this whole-file meaning.

File reads accept regular UTF-8 text in recognized inventory formats, including formats without
precise declaration support. A parse error does not prevent delivering the captured text; the
file's extraction status remains separate from source delivery. Unknown extensions and binary
input remain ineligible. The same policy, snapshot, hash and input limits apply as for definitions.

## Read explicit line excerpts

```sh
reposcout read . --range src/invoices.py 1 8 \
  --expect-hash src/invoices.py '<SHA256>' --budget 2048 -f json

# Follow exact current ranges reported by a working-tree change query.
reposcout read . --range src/policy.py 1 1 --snapshot worktree \
  --expect-hash src/policy.py '<CURRENT_SHA256>' -f json
```

`--range FILE START END` is repeatable and can share a batch with symbols, lines and complete
files. START and END are positive, inclusive, one-based physical line numbers, with START <= END.
Unlike `--line`, this explicitly selects an excerpt and does not expand to a containing declaration.
The result uses `selection: "range"`, `requested_range: {"start": START, "end": END}`, no fabricated
`definition`, and a reference to exact captured source bytes. A `complete` range result means the
requested excerpt is complete, not that it is a complete declaration or all relevant module context.

Physical lines are separated by LF. CRLF bytes and an unterminated final line are preserved.
A terminal LF does not create an extra empty line; an empty file has no selectable line. If either
bound lies beyond the file, the result is `not-found` with no clipped prefix. Invalid zero,
reversed or unrepresentable numeric bounds are usage errors. Oversized output is explicitly omitted;
RepoScout never reduces the requested bounds to make them fit.

Ranges share source chunks with overlapping explicit selections. `requested_range` retains the
individual request even if the shared chunk is larger because other targets overlap. Normal
failure/omission results retain it; the smallest status-only budget fallback may omit it together
with the other selection metadata. Disjoint ranges do not pull intervening lines into source.

Choose ranges from known source locations, prior output or a task-specific bounded exploration.
For example, a short module preamble can expose imports, but it does not prove that all bindings
occur there. Follow only actual delivered evidence. Keep hashes and matching snapshots on later
reads; when changes prove a retained body unchanged, fetch the changed binding instead of the body.
`plan` and `consumers` still accept declaration seeds, not explicit source-range selectors.

## Choose the source snapshot

```sh
# Worktree remains the default.
reposcout read . --symbol src/service.ts Service.start -f json

# Read exactly the staged version, including its body-free outline.
reposcout read . --snapshot index --symbol src/service.ts Service.start -f json
reposcout read . --snapshot index --outline src/service.ts -f json

# Read a definition from a Git revision, even if its file was deleted in the worktree.
reposcout read . --snapshot HEAD --symbol src/deleted.ts OldHandler -f json
```

`--snapshot` applies to every target in the call. The case-sensitive names `worktree` and `index`
select the current worktree or the captured Git index; any other value is a Git revision resolved
once to a tree object ID. To name a branch that collides with a reserved name, use its qualified
ref, such as `refs/heads/index`.

The returned file snapshot identifies its content side and the pinned tree revision where
applicable. Its SHA-256 identifies the captured file bytes. Use the path belonging to that side:
a Git-detected rename does not make an old path or old symbol name interchangeable with its new
one. Missing, unsupported or unreadable index/base content never silently falls back to worktree
bytes. Existing `--expect-hash` checks apply to the selected snapshot. In the shared query API, ref
aliases resolving to the same tree and path share file identity and hash expectations after
pinning; conflicting expectations are rejected rather than allowing an alias to bypass them.

Tree and index reads validate the selected Git entry's regular-file mode and blob identity.
Missing directories or symlinks in current source-parent paths do not redirect or invalidate that
historical content. Git symlinks and submodules remain unavailable, worktree reads retain
no-follow traversal, and current ignore/exclusion policy, hash expectations and input limits
apply to every snapshot.

## Select changed definitions

```sh
# Identify definitions touched by all uncommitted changes, without source bodies.
reposcout changes . --working -f json

# Read complete old/new definitions needed to inspect the staged change.
reposcout changes . --staged --source --budget 4096 -f json

# Compare a revision directly with captured worktree content.
reposcout changes . --since main -f json
```

`changes [PATH]` targets a directory or an existing file, defaults to `.`, and requires exactly one
diff scope. Use a directory scope for a file deleted from the worktree. It
uses the `agent` profile by default. The content pairs are:

| Scope | Old side | New side |
|---|---|---|
| `--working` | `HEAD` tree | Captured worktree, including staged, unstaged and untracked changes |
| `--staged` | `HEAD` tree | Captured index |
| `--since REF` | Resolved `REF` tree | Captured worktree |

`--since` compares the specified revision directly, not its merge-base with the current branch.
Git supplies candidate paths and detected renames. Exact changed hunks, definition facts and any
returned source are derived from the same captured buffers. Source remains absent unless
`--source` is explicit; this option uses the same complete-response token/byte budget as `read`.

Selection distinguishes directly changed innermost definitions, wrapper-only changes, ranges
outside supported declarations, ambiguous same-line siblings and ranges left unprocessed by a
work limit. Direct and wrapper-only candidates share one innermost comparison: a direct hit uses
its declaration span, while a wrapper-only hit uses its source span. Editing one declaration
does not mark a sibling merely because they share a retrieval wrapper. A change to their shared
wrapper header can retain multiple equally specific candidates as ambiguous. Multiple hunks
touching one definition are deduplicated. Zero-length insertion or
deletion anchors retain their old/new coordinates; they are not invented changed lines on an
empty side. Deleted definitions can be selected and read from the old side.

A rename crossing the selected target can have an old or new side outside scope. That side is
explicitly marked and is not read; it is not treated as an actually absent file. Conflicted index
entries, missing blobs, binary content and unreadable sources remain distinct capture outcomes.
Git capture accepts regular-file modes, not symlink or submodule targets.

A changes query captures at most 32 changed-file pairs, or the stricter configured file limit,
with up to 64 file-side captures. Both sides share 32 MiB total and an 8 MiB per-file ceiling;
stricter configured limits, including the Git-blob limit, still apply. These are source-capture
limits: Git candidate and rename discovery can perform additional I/O outside that 32 MiB
allowance. At most 4,096 hunks per file pair and 1,000,000 mapping-work units per side are
processed; omitted hunks and unprocessed ranges remain explicit. At most 128 derived result
targets proceed to metadata/source admission. `requested_targets` still counts every derived
target; `omitted_targets` includes both this projection cap and token/byte omissions. Capture gaps, mapping or
work-limit gaps and output omissions remain distinct. A large changeset is not presented as
fully covered merely because its bounded result fits. Output ordering and source deduplication
retain the content side: identical paths or line numbers in different snapshots are not the same
source chunk.

Neither snapshot retrieval nor changed-definition selection performs semantic rename guessing.
Separate [task queries](task-queries.md) provide lexical search, definition plans and conservative
consumer lookup. Ordinary `locate`, scouting and context output keep their
body-free defaults.

## Add definitions to an existing change report

```sh
reposcout --working --change-summary --changed-definitions -f json .
reposcout --staged --review=deep --changed-definitions -f json .
```

`--changed-definitions` adds the body-free `definition_changes` block to a change-summary or review
with exactly one diff scope. It supports table, JSON, Markdown and NDJSON; SARIF, DOT and Mermaid
are rejected. It conflicts with `--agent-summary` and `--baseline-ready` rather than silently
omitting the requested evidence.

The embedded block has its own fixed 4,096-token and 16,384-byte budget, measured on its compact
JSON representation. This does not cap the complete surrounding report. Existing parent
projection limits retain their meaning; `definition_changes` carries separate capture, selection
and output accounting. The block captures its source independently after the surrounding scan.
Its own diff, spans and source identity agree, but a live edit can make its captured content differ
from earlier parent-report metadata; the combined report is not an atomic snapshot. Use the
separate `changes --source` command when source is needed.


## Inspect body-free declarations when useful

```sh
reposcout read . --outline src/service.ts -f json
reposcout read . --outline src/service.ts --outline src/client.ts -f json
```

`--outline` conflicts with `--symbol`, `--line`, `--file` and `--range`; it is not a prerequisite for them. It returns
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
A full definition, explicitly requested file or exact range that does not fit is omitted with an
explicit reason. An explicit range is an excerpt; other selectors never silently become excerpts. Ambiguity retains at most eight candidates rather than choosing the first
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

Each requested file side is captured once per invocation. A worktree batch is not an atomic
snapshot across files, and files can change afterward. Git revisions are pinned to tree object
IDs and the index is captured for the query. A file's hash, diff ranges, spans and returned source
always refer to that same captured content. A local analysis-cache hit does not mean the agent still has
the source in its context.

## Coverage and discovery boundaries

For explicit `read` targets, source-input limits are separate from output limits: at most 8 MiB
per file, 32 MiB across the query and 32 files. Changed-definition queries use the side/pair limits
described above. Stricter configured limits also apply. Explicit targets retain the existing
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
| C# | `class`, `enum`, `function`, `interface`, `method`, `property`, `type` |
| GDScript | `class`, `constant`, `enum`, `method`, `property`, `signal` |
| Godot Shader | `function` |

Godot Scene, Resource and Project data and other generic inventory formats do not receive precise
source-definition support. A supported kind still needs a valid, safely retrievable range in the
particular file; the file's extraction state and result status qualify that fact.

## Interpret machine results

An explicit-read report identifies itself with `kind: "source_query"` and `mode: "source"` or
`"outline"`.
It keeps file facts, target outcomes and source content separate:

- `files` identifies selected file sides with `id`, `path`, `snapshot`, optional `language`, captured `sha256`,
  extraction status and available declaration/source-definition counts.
- `results` associates each one-based input `target` with a `status`, optional file/definition/source
  references,
  and bounded candidates with total/omitted counts. The `selection` reason names exact qualified or
  simple-name matching, innermost-line selection, a file outline, or `file` for complete-file reads.
  File and range results have no fabricated `definition`; their `source` references the captured
  chunk. Range results add `selection: "range"` and optional `requested_range` independently of
  merged source-chunk spans.
- `sources` contains shared chunks with `id`, `file`, `span` and `content`. A result references its
  chunk instead of repeating the same source for each target. File and chunk IDs are stable
  references within the response, not offsets into the arrays.
- `requested_targets` and `omitted_targets` preserve target accounting when the output budget
  removes detailed responses. If the root path is omitted to fit, `root_omitted` says so.

Changed-definition reports use `kind: "change_query"` and `mode: "changes"` or `"changes-source"`.
Here target IDs enumerate derived selections and gap records, not caller-supplied selectors.
Their top-level `change` object records `scope`, `base`, `current`, changed-file admission
(`total_files`, `processed_files`, `omitted_files`), mapped definitions, unmapped ranges,
unavailable sides, and total/omitted hunks and unprocessed ranges. These counts describe capture
and mapping before output admission; `omitted_targets` separately describes output omissions.

Each changed result can carry `change` evidence with `side` (`base` or `current`), Git
`file_status`, optional counterpart path, `reason`, direct `ranges`, separate `wrapper_ranges`
and an ambiguity flag. A `changed` result may represent a definition or a file change without
changed lines, such as a pure path rename; inspect its evidence and optional definition.
Ambiguous changed-line ownership is marked on evidence and is not an exact unique match.
Snapshot objects use `kind: "worktree"`, `"index"`, `"tree"` or `"empty"`; tree snapshots carry
`revision` with the pinned tree OID. `empty` represents an absent base tree.

A definition's `declaration_span` determines selection; optional `source_span` describes the
complete retrievable range, which can include a static wrapper. Spans use half-open byte offsets
and inclusive one-based line bounds. Empty file chunks use byte range `0..0` and line bounds `1..1`.
The optional `signature` is body-free metadata, not a
substitute for a complete `sources[].content`.

| Result status | Interpretation |
|---|---|
| `complete` | A complete selected source range was delivered |
| `outline` | Body-free declaration information was returned |
| `changed` | Body-free changed-definition or file-change evidence was returned |
| `unmapped` | Changed ranges lie outside extracted declarations |
| `binary`, `conflict` | Captured content is binary or the index entry is conflicted |
| `ambiguous` | Multiple identities match; inspect candidate totals and omissions |
| `not-found` | No matching declaration, or the requested physical-line range extends beyond captured text |
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
the selected total budget. NDJSON emits one compact `source_query` or `change_query` record,
according to the command, followed by a newline.
JSON and NDJSON preserve source bytes after JSON string decoding. Table and Markdown present
human-readable metadata and complete source ranges: newlines and tabs remain literal, while
other control characters, including carriage returns, are visibly escaped. Use JSON or NDJSON when
the exact decoded source content is needed.

Existing structured CLI errors remain available through `--error-format json`. Invalid options or
roots, unrecoverable source-loading or analysis failures, token-counter initialization or
serialization failures, and an unavoidable status envelope that cannot fit produce an error
rather than a successful partial document. On non-Unix platforms the command returns
`read is available only on Unix platforms` or `changes is available only on Unix platforms`
before reading source.

Choose the entry point already available:

- Known file and symbol or line: request the definition directly if its source is needed.
- Need a known small binding/registration span: use `read --range` with its snapshot/hash.
- Need complete module context: explicitly request `read --file` for the relevant files.
- Need a file's declaration surface: request its body-free outline from the relevant snapshot.
- Known diff scope: use `changes` for changed definitions and request `--source` only when needed.
- Unknown symbol location: use `locate`, lexical `find` or ordinary text search, then read only the
  selected definition when necessary.
- Need a bounded multi-file reading plan: use context planning and inspect its evidence before
  reading additional source.

RepoScout executes no model, compiler or test command for retrieval. Debug logs retain their
source-free contract. Successful retrieval proves the explicitly selected source was returned completely;
it does not prove that the definition is relevant or sufficient for the whole task.
