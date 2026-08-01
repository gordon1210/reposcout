# Context planning

Use this reference when a task needs a bounded reading plan, focus paths, declaration outlines,
token-budget decisions, or interpretation of `work_scope`.

## Request a bounded plan

```sh
reposcout -f json --summary --profile agent \
  --focus <file-or-directory> --context-budget 24000 --context-max-files 15 <scan-root>
```

`--focus` enables context planning. Use the smallest scan root that contains the focus and its
likely neighbors. For an untrusted checkout, retain `--profile safe`; use `full` only when the
reading decision also needs duplication or churn.

Follow `context.files` in rank order and open the selected source with ordinary repository tools.
RepoScout never embeds source bodies. First-class files can carry bounded declaration outlines;
an oversized explicit focus may retain an outline without pretending its source fits the budget.

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

`summary.skip_candidates` identifies generated, minified, or vendored files that are usually poor
reading choices. An explicit focus can still retain one when the task requires it.

Request a bounded graph neighborhood only when relationships themselves are the question:

```sh
reposcout -f json --summary --graph-focus <file> \
  --graph-direction both --graph-depth 2 <scan-root>
```

Report unresolved imports, parse errors, configuration errors, unmatched focus, and capped graph
results. Broaden the context budget or request detailed output only when omissions or confidence
gaps prevent the next reading decision.
