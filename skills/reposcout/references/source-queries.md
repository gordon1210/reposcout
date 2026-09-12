# Explicit source queries

Use this reference when a file and definition or line are already known, or when a file's body-free
declaration surface answers the next question. Do not run a scout, outline or locate merely as a
prerequisite. If the exact complete small span is already known and no snapshot, hash or coverage
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
`--line` pairs in their input order; budget admission and one-based target IDs follow that order.
Interleaving flags does not interleave results. `--outline` is a separate mode.

The complete rendered response, including headers, metadata,
candidates, omissions and newline, must fit both limits. The token budget defaults to 4,096
(256–65,536); the byte budget defaults to 65,536 (1,024–1,048,576). The effective token encoding is
`o200k_base` or `cl100k_base`. Pretty JSON costs count too.

A full definition that cannot fit is explicitly omitted. Do not interpret an omission as an empty
definition or ask for a larger budget unless the missing source affects the next decision. There
is no partial-source mode. JSON and NDJSON preserve the source after string decoding; NDJSON
emits one compact record. Human formats visibly escape controls other than newlines and tabs,
so use machine output when exact decoded source is required. Ambiguous selections expose at
most eight candidates; choose based on
actual task evidence rather than silently taking the first.

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

`--outline` is body-free and conflicts with `--symbol` and `--line`. All files share a maximum of
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
