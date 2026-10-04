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

`hunk_status` distinguishes available, partial and unavailable change analysis. When either
side's content was not captured (for example because of a size or input budget), `hunks` is `null`;
a numeric zero means the comparison was performed and found no text hunks. Added/deleted files
compare the existing side against empty text. Interpret `hunks_omitted` only when `hunks` is a number:
it counts discarded computed hunks and remains zero when analysis could not run.
Each side's `mapping_status` separately qualifies its changed ranges and declarations,
independently of source `extraction` status.
`totals.changes_without_hunks` counts retained change pairs without computable hunks,
including pairs for which only one side fit the shared capture budget.

Pure insertions or deletions can leave one side without changed lines. For existing declarations,
the query maps unchanged header-start lines across the captured hunks and requires matching names
and kinds before retaining the opposite declaration. `counterpart_definitions` counts these
additions; `unprocessed_counterparts` reports bounded mapping work that could not finish.
`ambiguous_counterparts` counts ambiguous matches, which retain partial mapping status.
`counterpart_seed_mapping_incomplete` marks a partial or unavailable mapping on the supplying
side; its unknown missing declarations are not counted as finished or as counterpart-work omissions.
The receiving side retains partial mapping and conservative file-based reference evidence.
A newly added/deleted header-start line or absent file side
does not acquire a guessed counterpart. `ranges` continues to describe actual changed lines.

Each revision has its own graph. File-import dependents include direct and transitive reverse
edges; changed files' direct dependencies are also candidates. Relations retain direction and
resolver provenance. Concrete symbol-reference evidence is separate from file-import impact and
uses the existing conservative Rust and JS/TS/TSX resolvers. These are potential static effects, not a
promise that every consumer breaks or that runtime consumers have been found. A deleted file can
still expose its old importers in the base graph.

Symbol references expose `change_basis`: `changed-definition` means a mapped changed declaration
touches the reference. When declaration mapping is partial or unavailable on a captured changed file,
`changed-file` preserves its resolved references without claiming that their declarations changed.
These candidates use `file-reference-source` / `file-reference-target` roles; precise mappings use
`concrete-reference-source` / `concrete-reference-target`. A mapped changed declaration takes
precedence when both bases apply. Mapping status and the actually mapped declarations remain
unchanged; file-based evidence never invents a changed declaration.

Resolver-configuration scope is a directory-based hint, not a proven call relationship. Test
evidence distinguishes filename conventions and Rust inline-test syntax; it is neither an
executed test result nor measured coverage. Existing graph and language limitations apply.
Changed root resolver configurations can therefore suggest files throughout the repository.
Unchanged resolver configurations support resolution but are not automatically added to the
reading list: being read by a resolver is not evidence of relevance to this change.
The shared graph's path convention cannot represent a literal Unix backslash unambiguously.
Such paths retain change, source and cost evidence but are excluded from graph inputs and counted
in `coverage[].unsupported_graph_paths`; they can never alias a slash-separated path.

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

Configured input limits may narrow these ceilings. Changed sides are captured first;
remaining paths alternate between base and head. Rename detection is bounded separately and
reports when skipped. Its work limit applies to the repository-wide comparison before the
changed-path filter, preserving bounded detection of cross-boundary renames. Hunks and declaration
mappings reuse the existing bounded change mapper. The duration limit is cooperative: checks
between bounded analysis units and after graph construction, call resolution and rendering abort
an expired query with an error, including when the deadline expires during capture. Duration
exhaustion does not return a partial report. A single parser, Git or graph operation is not preempted.

Read `coverage` and omission counters before interpreting an empty result. Inventory truncation,
excluded/unavailable inputs, unsupported analysis, parse/configuration errors and unresolved
relationships can hide impact. Reported candidate costs describe the observed neighborhood, not
an upper bound on unknown consumers. `changes_not_analyzed` records change pairs omitted before capture separately from
`changes_omitted` (output projection). Other output omissions stay separate from capture gaps and
selection exclusions; totals survive output projection. Increase the output allowance for more
entries or use narrower changed-path scope. If even the status envelope does not fit, the command
fails rather than returning a misleading empty success.
Projection strategy 2 preserves compact change identities by reducing the largest declaration
lists, then range lists, before removing relations, context candidates or whole changes. Lists
retain deterministic prefixes and are reduced geometrically to bound projection work. Per-side
`definitions_omitted` and `ranges_omitted` count removed details; these output counts do not alter
`mapping_status` or analysis-gap counters. If whole changes still cannot fit, the largest compact
JSON entry is removed first. Omitted compact identities are then reconsidered against the actual
renderer and tokenizer before any details are restored; a byte-heavy but token-affordable identity
must not disappear merely because another path costs more tokens. The complete response must fit
both budgets in the selected format.
Previously trimmed changes are reconsidered for complete detail restoration, smallest first,
so removing an oversized entry can make room for a small change's ranges and declarations again.
Relations alternate between head and base so either side cannot consume the entire output limit
while the other has evidence; references touching changed definitions, type relations, file-based
references and imports follow that order within each side. File-based references prioritize
incoming cross-file references to incompletely mapped files, then outgoing references, then
same-file references.
Inventory truncation also includes unrepresentable unchanged path names. Unsupported inventory
counts refer to unrecognized formats, not just failed parsers; Markdown is recognized.

Table, Markdown, JSON and single-record NDJSON use the same facts and output budgets. Source and
diff text are absent by default. Human output includes blob IDs, changed ranges, mapping status,
context distances and predecessors, and concrete symbol/type evidence; JSON and NDJSON retain the
complete structured records. `--output` and `--debug-log` must be outside the worktree and its
Git metadata (including linked-worktree common metadata) to
preserve the source and policy being reviewed. Analysis cache writes use the ordinary external,
best-effort cache; `--no-cache` disables them.
