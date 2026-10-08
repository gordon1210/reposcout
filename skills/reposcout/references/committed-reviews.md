# Committed reviews

Use this reference when a committed comparison needs detail beyond the entry guide.
When the exact next source or comparison question
needs no structural impact, selection or coverage evidence, a targeted native diff/read can answer
it directly. Use the selected immutable commit/tree IDs for those diffs and reads; resolve mutable
refs once and retain those IDs, never substituting live worktree content. Two known refs or a tiny
diff do not waive needed consumer, import-binding or
registration evidence. Reuse retained source with its exact revision, path and line range, plus
hash when available, rather than rereading unchanged text solely to format final evidence.

For revision-local impact, coverage or a reading plan, prefer `-f table` for body-free
orientation when its revision/path/hash handles, relations and gap/omission summaries suffice
to choose changed-source and consumer reads:

```sh
reposcout review-context <directory> --base origin/main --head HEAD -f table
```

Use `-f json` for automation, exact coverage counters or configured budgets, full symbol or
wrapper spans, relation-endpoint identities, or ambiguous escaped paths. Table is a reading aid,
not a machine interface. Keep base/head handles separate and distinguish source-selection
exclusions from response omissions. Reuse sufficient retained evidence; no Table-to-JSON repeat
is needed unless a remaining question requires those structured fields.

This Unix query pins both local refs and captures source and resolver configuration from their
immutable trees. The default is an exact direct comparison; add `--merge-base` only for the
unique-merge-base comparison. Preserve those trees on follow-up reads; worktree content is not a
substitute. Use `--profile safe` for an unfamiliar or untrusted checkout.

Read coverage and output omissions before interpreting changes or impact. Check `hunk_status`,
per-side `mapping_status` and `totals.changes_without_hunks`: a captured side does not prove a
comparison with a missing side was analyzed. Literal Unix backslash paths retain source but have
an explicit `unsupported_graph_paths` gap. File imports, concrete symbol references and test hints
are distinct; test hints are not executed tests, and none proves runtime completeness. Unsupported
or unresolved calls are not absent consumers. Use focused native source evidence when mappings
or relations cannot answer the question.

For symbol references, `change_basis: "changed-definition"` identifies mapped declarations;
`"changed-file"` preserves references involving a file with incomplete declaration mapping and
does not identify which declaration changed. Pure insertions/deletions can retain counterparts
through unchanged headers even with empty opposite-side `ranges`. Check `counterpart_definitions`,
`ambiguous_counterparts`, `unprocessed_counterparts`, `counterpart_seed_mapping_incomplete` and
per-side `definitions_omitted` / `ranges_omitted` before interpreting missing detail.

`totals.candidate_tokens` counts whole files once per revision/path; unknown costs stay explicit.
Keep base, head and unified-diff costs separate, even when source hashes match. Preserve configured
tokenization; never choose it by model. Agent allocation and task splitting remain the caller's job.

Use `--context` only for an initial reading list; `--context-budget` and `--context-max-files`
override its configured limits. Bodies are opt-in: `--source` requests complete selected files,
`--diff` requests patches. All content shares `--budget` and `--max-output-bytes`; source selection,
capture gaps and output omissions remain separate. For follow-up source, use the reported tree
with `read --snapshot` and the reported SHA-256. A known request needs no scout or guide first;
consult [source-queries.md](source-queries.md) only for unresolved selector, identity or delivery
semantics.
For exact quotations, retain pinned literal source; use JSON source output when carriage returns
or control-character escaping would change the text.

Choose validation to resolve remaining uncertainty and satisfy the task's required checks. Keep
static proof, executed checks and suggested unexecuted witnesses distinct; additional replay
breadth is useful when it answers an unresolved question, not a prerequisite for every conclusion.
