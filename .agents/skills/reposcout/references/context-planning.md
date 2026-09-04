# Context planning

Use this reference when a task needs a bounded reading plan, focus paths, declaration outlines,
token-budget decisions, or interpretation of `work_scope`.

## Request a bounded plan

```sh
reposcout --agent-summary \
  --focus <primary-file> --focus <related-file-or-directory> \
  --context-budget 24000 --context-max-files 15 <scan-root>
```

`--focus` enables context planning and is repeatable. Put every related path already known for the
task into one invocation so all candidates compete within one shared token budget and file limit.
Do not run an independent full-budget plan for each known focus. Separate plans remain appropriate
for genuinely independent questions or a focus discovered only after the first decision.

Use the smallest scan root that contains the focuses and their likely neighbors. Prefer exact files
or the narrowest meaningful directory: a directory focus expands to every analyzed file beneath
it and can consume much of the bounded plan. For an untrusted checkout, retain `--profile safe`;
use `full` only when the reading decision also needs duplication or churn.

Treat the agent view's `context.direct_evidence.entries` and `expand_if_needed.entries`—or
`context.files` in ordinary output—as a ranked candidate set, not a checklist to read in full.
Open only the highest-ranked source needed to test the next hypothesis, then stop when the task has
enough evidence. `context.budget.selected_tokens` in the agent view (`context.selected_tokens` in
ordinary output) is the potential source cost if every selected body were read; it is not the size
of RepoScout's output or a requirement to spend the whole budget. RepoScout never embeds source
bodies.

Agent-summary output keeps direct selection evidence, a small expansion tier, aggregate outline
counts, and bounded outline-only seeds without declaration objects. Use ordinary `--summary` when
complete context metadata is needed, and drop `--summary` only when bounded declaration signatures
are needed. An oversized explicit focus remains visible without pretending its source fits the
budget. Inspect `context.outline_only.entries` before the direct tier so such a seed is not missed.
Without a focus or change seed, `direct_evidence` is empty by design and the expansion tier is only
a bounded orientation shortlist; add a focus for a task-specific reading plan. If later evidence
requires another plan, do not reread an unchanged file merely because it appears again.

## Project only the next decision

Do not pass the complete scan report into model context when the native bounded view answers the
question. This single call retains coverage, focus resolution, aggregate budget facts, up to five
direct-evidence files, and up to three lower-priority expansion candidates:

```sh
reposcout --agent-summary \
  --focus <primary-file> --focus <related-file-or-directory> \
  --context-budget 24000 --context-max-files 15 <scan-root>
```

When coverage is already known and only the immediate reading order is needed, request a short
human-readable list:

```sh
set -o pipefail

reposcout --agent-summary \
  --focus <primary-file> --focus <related-file-or-directory> \
  --context-budget 24000 --context-max-files 15 <scan-root> \
  | jq -r '.context.direct_evidence.entries[]
      | [.path, (.tokens | tostring), ([.evidence[].role] | join("; "))]
      | @tsv'
```

Use ordinary summary JSON only when the shortlist or confidence gaps require complete omission
details unavailable in the native view:

```sh
set -o pipefail

reposcout -f json --summary --profile agent \
  --focus <primary-file> --focus <related-file-or-directory> \
  --context-budget 24000 --context-max-files 15 <scan-root> \
  | jq -c '{
      budget: (.work_scope.context | {
        budget_tokens, selected_files, selected_tokens,
        omitted_files, omitted_tokens, skipped_files, truncated
      }),
      outline_only: [(.context.outline_only // [])[]
        | {path, source_tokens, reason}],
      omissions: (.context.omitted // []),
      confidence: .work_scope.confidence
    }'
```

Here, `context.budget.plan_omitted_*` in agent-summary describes the underlying token/file plan,
while each bounded list's `omitted` value describes only entries hidden by the projection. Never
merge those completeness domains. Before concluding that no dependency or dependent exists,
compare `context.evidence.graph_covered_seed_files` with `graph_eligible_seed_files`; an uncovered
seed turns empty relationship evidence into an unknown, not a clean zero.

Do not pipe every selected path into `cat`, `xargs`, or an equivalent bulk reader. That converts a
maximum planning budget into mandatory context spend and discards the ranking's purpose.

## Read `work_scope` before the file list in detailed output

Interpret its blocks in this order:

1. `basis` and `inventory`: confirm repository, focus, or diff scope plus observed source files and
   tokens.
2. `seeds`: verify resolved focus/change paths, unmatched inputs, full totals, and bounded path
   omissions.
3. `context`: compare the requested budget with selected, outline-only, omitted, skipped, and
   truncated totals.
4. `impact` and `structure`: use known dependents, matching-test estimates, and observed graph
   components as topology evidence—not proof of independent work streams.
5. `confidence`: identify unavailable analyzers, parse/import/configuration gaps, and incomplete
   bounded analysis before trusting apparent absences.

Counts describe the full observed result even when path arrays are capped. Never ignore an omitted
counter merely because the visible list looks complete.

## Interpret selection evidence

Selection can use focus proximity, repository instructions, manifests, entrypoints, graph
neighbors, matching tests, risk, churn, and complexity. Treat each reason according to its stated
confidence. `high` means direct syntax/configuration evidence; `partial` can be heuristic or
transitive.

`signals.skip_candidates` in agent-summary (or `summary.skip_candidates` in ordinary output)
identifies generated, minified, bundled, or vendored files that are usually poor reading choices.
An explicit focus can still retain one when the task requires it.

Request a bounded graph neighborhood only when relationships themselves are the question:

```sh
reposcout -f json --summary --graph-focus <file> \
  --graph-direction both --graph-depth 2 <scan-root>
```

Report unresolved imports, parse errors, configuration errors, unmatched focus, and capped graph
results. Broaden the context budget or request detailed output only when omissions or confidence
gaps prevent the next reading decision.
