# Explicit source queries

Use this reference when a file and definition or line are already known, or when a file's body-free
declaration surface answers the next question. Do not run a scout, outline or locate merely as a
prerequisite. A normal short read may already be sufficient; avoid rereading unchanged source that
is still available in model context.

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

## Request outlines only when needed

```sh
reposcout read . --outline src/service.ts --outline src/client.ts -f json
```

`--outline` is body-free and conflicts with `--symbol` and `--line`. All files share a maximum of
100 returned declarations and the same output budget. Inspect omissions; this is not guaranteed
to list every declaration. Do not request an outline before a direct read when the target is
already known.

## Keep identity and coverage honest

The command reads current worktree content, not a Git index or base revision. Source ranges and
returned bytes belong to the same captured file content. Each file is captured once, but the batch
is not an atomic snapshot across files or a Git snapshot. The returned SHA-256 identity can guard
a later
read with repeatable `--expect-hash <file> <sha256>`; only selected files may be named. A stale
expectation returns no source for that file. A hash is not a guarantee that the file remains
unchanged after the command finishes.

Input limits are distinct from output limits: 8 MiB per file, 32 MiB total and 32 files, or stricter
configured limits. The command defaults to the `agent` profile; use `--profile safe` for an
untrusted checkout. Explicit paths retain ignore, exclusion, target and no-follow policy. Unsupported
languages/declaration kinds, parser/extraction gaps, policy-ineligible input, stale content and
output omissions do not prove that a definition is absent. Generic format recognition is not
precise definition support.

A complete returned definition is syntactically complete within the supported contract. It can
still need related types, imports, callers or tests for the task. Read further only when the next
hypothesis requires it. Local cache reuse saves local work; it does not prove the agent retains
source and does not itself demonstrate lower model-token consumption.
