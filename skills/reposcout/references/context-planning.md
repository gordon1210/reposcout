# Context planning

Use this reference when a task needs a bounded reading plan, focus paths, declaration outlines,
token-budget decisions, or interpretation of `work_scope`.

## Request a bounded plan

```sh
reposcout -f json --summary --profile agent \
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

Treat `context.files` as a ranked candidate set, not a checklist to read in full. Open only the
highest-ranked source needed to test the next hypothesis, then stop when the task has enough
evidence. `selected_tokens` is the potential source cost if every selected body were read; it is
not the size of RepoScout's output or a requirement to spend the whole budget. RepoScout never
embeds source bodies.

Summary output keeps file-level selection evidence and aggregate outline counts but omits detailed
declaration objects; `outline_details_omitted` reports when at least one such object was removed.
Drop `--summary` only when bounded declaration signatures are needed. An oversized explicit focus
remains visible without pretending its source fits the budget. If later evidence requires another
plan, do not reread an unchanged file merely because it appears again.

## Project only the next decision

Do not pass the complete scan report—or complete bounded objects such as `diagnostics` with their
sample arrays—into model context when a smaller projection answers the question. Choose one of the
following projections before running RepoScout; they are alternatives, not sequential scans.

This compact default retains coverage, focus resolution, aggregate budget facts, and at most eight
ranked candidates:

```sh
set -o pipefail

reposcout -f json --summary --profile agent \
  --focus <primary-file> --focus <related-file-or-directory> \
  --context-budget 24000 --context-max-files 15 <scan-root> \
  | jq -c '{
      coverage: .work_scope.confidence,
      focus: .work_scope.seeds.focus,
      budget: .work_scope.context,
      files: [.context.files[:8][] | {path, tokens, reasons}]
    }'
```

When coverage is already known and only the immediate reading order is needed, request a short
human-readable list:

```sh
set -o pipefail

reposcout -f json --summary --profile agent \
  --focus <primary-file> --focus <related-file-or-directory> \
  --context-budget 24000 --context-max-files 15 <scan-root> \
  | jq -r '.context.files[:5][]
      | [.path, (.tokens | tostring), (.reasons | join("; "))]
      | @tsv'
```

Use the omission projection instead only when the shortlist or confidence gaps prevent the next
decision:

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

Do not pipe every selected path into `cat`, `xargs`, or an equivalent bulk reader. That converts a
maximum planning budget into mandatory context spend and discards the ranking's purpose.

## Read `work_scope` before the file list

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

`summary.skip_candidates` identifies generated, minified, bundled, or vendored files that are
usually poor reading choices. An explicit focus can still retain one when the task requires it.

Request a bounded graph neighborhood only when relationships themselves are the question:

```sh
reposcout -f json --summary --graph-focus <file> \
  --graph-direction both --graph-depth 2 <scan-root>
```

Report unresolved imports, parse errors, configuration errors, unmatched focus, and capped graph
results. Broaden the context budget or request detailed output only when omissions or confidence
gaps prevent the next reading decision.
