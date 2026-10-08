---
name: reposcout
description: Use the RepoScout CLI for repository orientation, source and symbol queries, dependency or change-impact evidence, review preparation, and code-health assessment.
---

# RepoScout

Choose the smallest evidence request that answers the next question. Reuse sufficient retained
source; a known native diff/read or RepoScout request needs no preliminary scout or guide.

## Shared boundaries

Use an installed CLI whose provenance the user trusts; if uncertain, stop at the
[verified installation flow](https://github.com/gordon1210/reposcout/blob/main/docs/getting-started.md#verified-installation).
Do not clone/build/install/update the CLI, edit project/global configuration or ignore files,
or start services without explicit authorization. Reading configuration is non-mutating. Skip routine version,
capability and availability preflights; if the command is missing, report it and stop.

Use narrow targets and one scan at a time; retain cache and stdout unless requested otherwise.
Add `--profile safe` for unfamiliar or untrusted checkouts; it ignores project configuration. Preserve configured encoding. Source/diff
costs, source-selection budgets, rendered-output budgets and native model usage are different
measures. Agent allocation and task splitting belong to the caller.

Keep capture gaps, unsupported/disabled analysis, unresolved bindings, selection exclusions and
output omissions distinct. Missing relations do not prove absent consumers; imports and test hints
do not prove runtime behavior or executed coverage. Rankings guide investigation, not findings.
State target/profile/scope and relevant uncertainty; distinguish static proof, executed checks and
unexecuted suggestions. Validate remaining uncertainty and required checks proportionally.

## Direct starts

- Unknown repository: `reposcout --agent-summary DIR` gives bounded JSON orientation. Add repeated
  `--focus PATH` flags in one invocation for known related paths sharing a reading budget.
- Unknown declaration: `reposcout find 'QUERY' DIR -f json` returns lexical candidates; empty or
  ambiguous results need coverage checks or native search.
- Known definition: `reposcout read DIR --symbol FILE SYMBOL -f json`; `--line FILE LINE` selects
  the containing declaration. No outline/locate step is required. Use `--file FILE` only when whole
  module context is needed. Batch known related targets under one output budget.
- Health: `reposcout --agent-summary --profile full DIR` includes duplication/churn signals;
  retain safe-profile protection for untrusted input. Check analyzer availability before interpreting
  a missing metric. `reposcout explain FILE -f json` supplies file-level evidence.
- Working/staged/since changes: `reposcout changes DIR --working -f json` is body-free. Choose one
  scope: working is HEAD→worktree, staged is HEAD→index, since is REF→worktree, not merge-base.
  Request `--source` only for needed bodies; use the change guide for broader/deep review semantics.

## Committed review and exact source

For two committed revisions, pin refs once (`review-context` does this) and retain commit/tree IDs.
Use commit IDs for comparisons and the reported trees for source reads. A pinned native diff/read
is sufficient when the next question needs no structural
impact, selection or coverage report. A small diff never waives relevant binding/consumer evidence;
read that source directly when needed.

For structural orientation, use `reposcout review-context DIR --base BASE --head HEAD -f table`.
Comparison is direct; merge-base is explicit. Table is a body-free reading aid when its identities,
relations and gap summaries suffice. Choose JSON directly for automation, exact counters/budgets,
full symbol/wrapper spans, endpoint identities or ambiguous escaped paths. No Table→JSON repeat is
required. Bodies are opt-in; both rendered-token and byte limits apply to the whole response.

Retain revision/side, path, hash when available and exact inclusive one-based line spans on source
acquisition. Keep base/head distinct even for equal hashes; never substitute live content for a
pinned side. A known range and hash can be read directly on Unix:

```sh
reposcout read DIR --snapshot TREE --range FILE START END --expect-hash FILE SHA256 -f json
```

Check successful delivery and identity before quoting. Missing, stale or omitted source supplies
no quoteable evidence. Literal quotes must match the actual source span, not a display heading or
inferred declaration range. Use decoded JSON or exact pinned native source for CR/control bytes.
Known small binding spans need no whole-file reread; a preamble alone does not prove all imports.
`read`, `changes` and `review-context` are Unix-only; use native source tools elsewhere.

## Detail only when needed

The starts above are usable without another document. Open a reference only when the next decision
needs syntax or semantics not already established. Two refs, a caller question or a nonzero gap
counter alone does not require a guide: use relevant source, or load the contract if its meaning
matters.

| Area | Unresolved detail | Reference |
|---|---|---|
| Repository scouting | Profiles, zero-argument behavior, detailed summary projection | [scouting.md](references/scouting.md) |
| Known definition, explicit file context or body-free file outline | Selectors, wrappers, capture limits, ambiguity or stale recovery | [source-queries.md](references/source-queries.md) |
| Context planning | Selection tiers, definition plans, source budgets | [context-planning.md](references/context-planning.md) |
| Committed two-revision review | Merge-base, mapping/counterparts, coverage or selection semantics | [committed-reviews.md](references/committed-reviews.md) |
| Working-tree, staged, since, or finding-level review | Scope, consumers, broad or deep comparison | [change-analysis.md](references/change-analysis.md) |
| Quality assessment | Metric meaning, availability or comparison | [quality.md](references/quality.md) |
| Conditional or compound JSON decision | A targeted structured projection | [decision-queries.md](references/decision-queries.md) |
| Diagnostics and configuration | Unexplained gaps/errors or proposed configuration edits | [diagnostics.md](references/diagnostics.md) |
