# Explicit source queries

Use this reference for source-selection syntax or semantics not already established by the
entry guide or retained evidence. A known read needs no additional guide, scout, outline or locate
as a prerequisite. If the exact complete small span is already known and no snapshot, hash or coverage
evidence is needed, prefer a targeted native read. Avoid rereading unchanged source that is still
available in model context.

`read`, `--outline`, snapshot reads and `changes` are Unix-only. Windows and other non-Unix builds reject them before source
I/O, without a fallback reader; use ordinary source tools on those platforms. Repository inventory
support is unchanged. When compatibility is uncertain, capabilities disclose
`source_query.available` and `source_query.platforms: ["unix"]`; no routine preflight is required.

## Read known definitions

```sh
reposcout read <target-directory> --symbol <file> <symbol> -f json
reposcout read <target-directory> --line <file> <line> -f json
```

Each selector takes two values. File paths are relative to the target directory; absolute paths
must remain within it using the canonical root or original target alias. The chosen directory is
resolved once; source components below that anchor are opened without following symlinks.
File selectors are UTF-8, at most 4,096 bytes and cannot contain `..`; symbols have a 1,024-byte
limit. Symbol matching is case-sensitive: exact qualified names take precedence
over exact simple names. Multiple matches in that tier remain ambiguous. A line selects the
innermost declaration's own span, including its attributes or decorators where supported.
Same-line siblings or multiple identities with equal spans remain ambiguous.

Batch all currently known related targets under one budget:

```sh
reposcout read . \
  --symbol src/service.ts Service.start \
  --line src/client.ts 27 \
  --budget 4096 --max-output-bytes 65536 -f json
```

There are at most 32 targets. CLI order is all `--symbol` pairs in their input order, then all
`--line` pairs, then all `--file` paths, then `--range` triples, preserving order within each group; budget admission
and one-based target IDs follow that order.
Interleaving flags does not interleave results. `--outline` is a separate mode.

The complete rendered response, including headers, metadata,
candidates, omissions and newline, must fit both limits. The token budget defaults to 4,096
(256–65,536); the byte budget defaults to 65,536 (1,024–1,048,576). The effective token encoding is
`o200k_base` or `cl100k_base`. Pretty JSON costs count too.

A full definition that cannot fit is explicitly omitted. Do not interpret an omission as an empty
definition or ask for a larger budget unless the missing source affects the next decision. Only `--range FILE START END` explicitly requests an excerpt; other selectors never silently
become partial source. JSON and NDJSON preserve the source after string decoding; NDJSON
emits one compact record. Human formats visibly escape controls other than newlines and tabs,
so use machine output when exact decoded source is required. Ambiguous selections expose at
most eight candidates; choose based on
actual task evidence rather than silently taking the first.

## Read missing module context explicitly

```sh
reposcout read . --file src/routes.py --file src/handler.py \
  --expect-hash src/routes.py '<SHA256_FROM_PRIOR_RESULT>' --budget 4096 -f json
```

Use this only when imports, registration tables or other surrounding file context are needed.
Batch discovered paths under one output budget; keep known hashes and the matching snapshot.
Prefer a definition read when it already answers the next question. No automatic import traversal
or source expansion occurs, and no new discovery call is required for an already known file.

`--file` returns complete captured text or an explicit budget omission. It can share a batch with
symbol/line selections; overlapping source is delivered once. Results use `selection: "file"`,
no `definition`, and a shared source reference. Recognized text formats without definition support
are eligible; extraction status remains separate from successful delivery. Empty files return an
empty source chunk. Unknown extensions, binary content and policy-ineligible paths remain rejected.
`plan --file` still selects declarations, not complete files.

## Read a small known span without repeating a whole module

```sh
reposcout read . --range src/policy.py 1 3 \
  --expect-hash src/policy.py '<SHA256_FROM_PRIOR_RESULT>' --budget 2048 -f json
```

`--range FILE START END` selects inclusive, positive, one-based physical lines. It preserves exact
captured LF/CRLF bytes and an unterminated final line; an empty file has no line, and terminal LF
adds no phantom line. Zero/reversed bounds are invalid; any bound past EOF returns `not-found`
instead of clipping. A requested excerpt that cannot fit is omitted whole.

Use ranges from prior source/change evidence or an explicit bounded caller exploration. A small
preamble is not a guarantee of complete import coverage. Keep snapshot/hash identity and request
only the new binding when retained source has a valid unchanged-content proof. Never infer a
binding from an unconnected name match. No import traversal or context expansion is automatic.

Results use `selection: "range"`, `requested_range: {"start": START, "end": END}`, no fabricated
definition and an exact source chunk. Overlapping explicit selections share a union chunk, so
its bounds may exceed one request. Normal omissions retain requested bounds; the minimal status
fallback may omit them. `complete` means the requested excerpt was delivered, not complete
function/module context. File/range selectors do not become `plan` or `consumers` seeds.

## Read the correct snapshot or select changed definitions

```sh
reposcout read . --snapshot index --symbol src/service.ts Service.start -f json
reposcout read . --snapshot HEAD --symbol src/deleted.ts OldHandler -f json
reposcout changes . --working -f json
reposcout changes . --staged --source --budget 4096 -f json
```

`--snapshot` applies to all explicit targets, including outlines. The case-sensitive names
`worktree` and `index` are reserved; another Git ref resolves once to a tree OID. Use the path and
symbol belonging to that side. Missing old/index source never silently uses worktree bytes.
Tree/index reads use Git's regular-file mode and blob identity, independent of live source-parent
symlinks or missing directories. Worktree reads retain no-follow traversal; current ignore and
exclusion policy still applies to every snapshot.

`changes [PATH]` takes a directory or existing file, defaults to `.` and requires one scope. Use a
directory scope for worktree-deleted files. Working compares
HEAD with worktree (staged, unstaged and untracked changes), staged compares HEAD with index,
and since compares the supplied ref directly with worktree, not a merge-base. Default output is
body-free; request `--source` only when source helps the next decision. The rendered response
uses the same budget as `read`.

At most 32 changed pairs and 64 side captures share a 32 MiB input limit and 8 MiB per-file bound,
or stricter configured limits. Git candidate/rename discovery can perform I/O outside that
capture allowance. At most 128 derived targets enter output admission; omitted-target counts
include this cap and byte/token omissions. Inspect capture gaps, directly mapped definitions, wrapper-only
changes, uncovered/ambiguous ranges, work-limit gaps and output omissions separately. A compact
result is not proof that a larger changeset was fully mapped. Do not infer semantic renames from
matching names; use Git-detected path evidence.

## Request outlines only when needed

```sh
reposcout read . --outline src/service.ts --outline src/client.ts -f json
```

`--outline` is body-free and conflicts with `--symbol`, `--line`, `--file` and `--range`. All files share a maximum of
100 returned declarations and the same output budget. Inspect omissions; this is not guaranteed
to list every declaration. Do not request an outline before a direct read when the target is
already known.

## Keep identity and coverage honest

Source ranges, exact diff hunks and returned bytes belong to the same captured file side. Each
side is captured once; worktree capture is not atomic across files. Git refs are pinned to tree
OIDs and index reads use the captured index. The returned SHA-256 identity can guard
a later
read with repeatable `--expect-hash <file> <sha256>`; only selected files may be named. A stale
expectation returns no source for that file. A hash is not a guarantee that the file remains
unchanged after the command finishes.

Explicit-read input limits are distinct from output limits: 8 MiB per file, 32 MiB total and 32 files, or stricter
configured limits. The command defaults to the `agent` profile; use `--profile safe` for an
untrusted checkout. Explicit paths retain ignore, exclusion, target and no-follow policy. Unsupported
languages/declaration kinds, parser/extraction gaps, policy-ineligible input, stale content and
output omissions do not prove that a definition is absent. Generic format recognition is not
precise definition support.

A complete returned definition is syntactically complete within the supported contract. It can
still need related types, imports, callers or tests for the task. Read further only when the next
hypothesis requires it. Local cache reuse saves local work; it does not prove the agent retains
source and does not itself demonstrate lower model-token consumption.

## Find candidates only for an unknown entry point

```sh
reposcout find 'retry delay duplicate payment' . --match all --limit 10 -f json
```

Names, paths, signatures, comments and bounded code provide lexical evidence; no bodies are
returned. Matching defaults to every deduplicated query term (`all`); `any` permits one. Unicode
lowercase and identifier splitting are lexical rules, not semantic task understanding. Exact
case-insensitive language/kind filters and stable field-based ranking keep the candidate set
predictable. Health risk and graph popularity are not relevance evidence.

Inspect search coverage separately from hit-limit and output-budget omissions. A truncated field
or unsupported file can hide a relevant declaration. Follow a chosen hit's structured read target
with its expected hash; after-edit mismatches need a fresh decision. A matching name can still be
ambiguous. Do not perform a new search when the source location is already known or reread retained
unchanged code merely because it appears in another result.
