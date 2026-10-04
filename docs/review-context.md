# Prepare a pull-request review

← [Documentation index](README.md)

`review-context` prepares revision-pinned evidence for a human or agent reviewing a PR: changes,
changed declarations, potential impact, test hints and source-token costs. It does not review the
correctness of the patch, call a model, allocate agents or split work. The caller makes those
decisions. Tokenization always uses the existing default, configuration or explicit `--encoding`.

```sh
# Compare two commits directly; HEAD is the default head
reposcout review-context . --base origin/main --head HEAD -f json

# Use the PR branch's unique merge base explicitly
reposcout review-context . --base origin/main --merge-base -f json

# Select an initial reading list; source remains opt-in
reposcout review-context . --base origin/main --context \
  --context-budget 24000 --context-max-files 20 -f json

# Include selected whole-file source and unified patches in one output allowance
reposcout review-context . --base origin/main --source --diff \
  --budget 16000 --max-output-bytes 131072 -f json
```

References must already exist locally; the command does not fetch, checkout or contact a PR
provider. It requires a Git worktree on Unix and UTF-8 changed paths. `PATH` must be a directory; a subdirectory restricts
the changed paths that seed the review, while importers elsewhere in the repository remain
eligible. Use [changes](source-queries.md) for worktree/index comparisons and existing
`--review=deep` for finding-level regressions.

## Identity and evidence

Both references resolve once to commits and trees. Direct comparison is the default. With
`--merge-base`, exactly one common base is required; requested and actual base commits are both
reported. Sources, hunks, declaration mappings, hashes and resolver configuration come from the
same immutable trees. Dirty source files, staged edits and live resolver configuration do not
enter either side. Current RepoScout configuration and ignore policy still govern eligibility.

The change inventory includes additions, deletions, renames, mode and type changes, hidden paths,
unsupported formats, binaries, symlinks and submodules. Inclusion in this inventory does not
grant permission to read excluded content. Each side reports its path, mode, object ID,
availability and captured SHA-256 where available. Symlinks and submodules are never followed.
Unavailable or unsupported declaration analysis retains raw change evidence when text capture
succeeds. Mode-only changes have no invented changed declarations.

Each revision has its own graph. File-import dependents include direct and transitive reverse
edges; changed files' direct dependencies are also candidates. Relations retain direction and
resolver provenance. Concrete symbol-reference evidence is separate from file-import impact and
uses the existing conservative Rust and JS/TS/TSX resolvers. These are potential static effects, not a
promise that every consumer breaks or that runtime consumers have been found. A deleted file can
still expose its old importers in the base graph.

Resolver-configuration scope is a directory-based hint, not a proven call relationship. Test
evidence distinguishes filename conventions and Rust inline-test syntax; it is neither an
executed test result nor measured coverage. Existing graph and language limitations apply.

## Token costs and optional reading list

`totals.candidate_tokens` counts complete captured candidate files once per revision/path.
Base and head are intentionally separate: a reviewer may need both. Unknown sizes are `null`
and counted separately, never silently treated as measured zero. Counts use the configured
tokenizer; there is no model detection or tokenizer switching.

`totals.diff_tokens` is the separate cost of captured unified patches. Adding it to source costs
describes reading both representations, including repeated text. These figures exclude harness
instructions, conversation history and model output. They are not a forecast of total agent
usage. The rendered response has its own hard token and byte limits.

`--context`, `--context-budget`, `--context-max-files` or `--source` enables a deterministic initial
reading list. It prioritizes changed sides, test candidates, direct neighbors and then more
distant candidates; ties use distance, path and side. A whole file must fit both the remaining
source-token allowance and file-count allowance. Every candidate retains a selection reason.
The defaults come from `[context]` configuration (32,000 tokens and 25 file sides unless changed).
CLI overrides keep the ordinary execution-profile and absolute safety limits.

Without these flags there is no selection. `--source` delivers complete selected files or omits
their bodies explicitly; it never cuts source mid-file. The source selection budget does not
grant extra output capacity. Selection totals describe the initial list before output projection.
Further definition reads can use `read --snapshot <reported-tree>` with the reported path/hash.

## Limits and omissions

| Bound | Default / ceiling |
|---|---|
| Rendered response | 4,096 tokens / 65,536 bytes |
| `--budget` | 256–65,536 tokens |
| `--max-output-bytes` | 1,024–1,048,576 bytes |
| Inventory | At most 10,000 paths per revision |
| Captured inputs | At most 10,000 file sides / 32 MiB shared by both revisions |
| Individual input | At most 8 MiB, additionally bounded by configured file/blob limits |
| Output entries | At most 100 changes, 100 candidates and 100 relations, then byte/token projection |

Configured input and duration limits may narrow these ceilings. Changed sides are captured first;
remaining paths alternate between base and head. Rename detection is bounded separately and
reports when skipped. Hunks and declaration mappings reuse the existing bounded change mapper.

Read `coverage` and omission counters before interpreting an empty result. Inventory truncation,
excluded/unavailable inputs, unsupported analysis, parse/configuration errors and unresolved
relationships can hide impact. Reported candidate costs describe the observed neighborhood, not
an upper bound on unknown consumers. `changes_not_analyzed` records input-limited change pairs separately from
`changes_omitted` (output projection). Other output omissions stay separate from capture gaps and
selection exclusions; totals survive output projection. Increase the output allowance for more
entries or use narrower changed-path scope. If even the status envelope does not fit, the command
fails rather than returning a misleading empty success.

Table, Markdown, JSON and single-record NDJSON use the same facts and output budgets. Source and
diff text are absent by default. `--output` and `--debug-log` must be outside the worktree and its
Git metadata (including linked-worktree common metadata) to
preserve the source and policy being reviewed. Analysis cache writes use the ordinary external,
best-effort cache; `--no-cache` disables them.
