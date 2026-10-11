---
name: reposcout
description: "Use for any question about a change between two commits, branches or a pull request: reviewing it, judging whether it is safe to merge or ship, explaining what changed, or finding defects, regressions and risks. Run `reposcout review-context . --base <base> --head <head> --profile safe --brief` before `git diff` or `git log`: one call returns the changed hunks, changed definitions, importers, call sites and docs that name them. Also for repo orientation, symbol lookup, change impact and code health."
---

# RepoScout

Every byte you load stays in context for the rest of the task. Make one targeted RepoScout call
instead of several native lookups, then read only the line ranges you still need.

## Boundaries

Use the installed CLI as is: do not install, build, configure it or edit ignore files. If the
command is missing, say so and continue natively. Add `--profile safe` for unfamiliar or untrusted
checkouts. `read`, `changes` and `review-context` are Unix-only.

## Review two commits (PR or branch review)

1. Run once:

   ```sh
   reposcout review-context . --base <base> --head <head> --profile safe --brief
   ```

   It pins both commits and prints, within 8k tokens by default: recurring token rewrites
   (`R1 old → new`), moved and renamed code with destinations, old spellings still in head, and the
   changed hunks with their changed definitions, direct importers and call/reference sites
   (`path:line`), docs naming changed definitions, and nonzero gaps. Hunks that only apply pure
   identifier renames are listed as sites; hunks tagged `[R1 R2]` show only their changed lines
   because every edit is a listed rewrite. Moved code keeps its text; check that the names it uses
   resolve the same at the destination. Read hunks marked `deviates from Rn` first: they break a
   recurring edit. Judge whether each rewrite belongs at its sites instead of fetching them. Run
   `git diff` only for omitted hunks, with the commands the brief prints.
2. Read only what the brief does not answer, as line ranges at the pinned commit, several ranges
   per command: `git show <head>:path | sed -n '40,90p'`, or `reposcout read . --snapshot <head>
   --range FILE START END -f json`. Avoid dumping whole large files.
3. Consumers and doc mentions are starting points, not a complete boundary. When gaps are nonzero
   or dispatch is dynamic (registries, string keys, config), search the pinned commit with
   `git grep -n PATTERN <head> -- DIR`. A listed consumer is a lead, not evidence: before
   concluding that nothing breaks, read each listed call site or usage line in context, or run the
   tests that exercise it, and cite what you checked.
4. RepoScout replaces lookups, not verification. Still run the task's permitted checks, and a
   minimal witness when you claim reachable behavior, as you would without it. A RepoScout call is
   static evidence, never an executed check.
5. Cite evidence as path plus line range at the pinned commit, quoting source lines verbatim
   without diff markers; for behavior-preserving claims cite both the base and the head lines.
   Keep static reasoning, executed checks and unexecuted suggestions separate.

## Other starts

- Unknown repository: `reposcout --agent-summary DIR`; add `--focus PATH` for known related paths.
- Unknown declaration: `reposcout find 'QUERY' DIR -f json`.
- Known definition: `reposcout read DIR --symbol FILE SYMBOL -f json`, or `--line FILE LINE` for
  the containing declaration.
- Working tree, staged or since-ref changes: `reposcout changes DIR --working -f json` (`--staged`,
  `--since REF`); add `--source` only for bodies you need.
- Health: `reposcout --agent-summary --profile full DIR`; `reposcout explain FILE -f json`.

Missing relations do not prove absent consumers; test hints are not executed tests.

## References

Open one only when a decision needs semantics not covered above.

| Area | Reference |
|---|---|
| Repository scouting | [scouting.md](references/scouting.md) |
| Known definition, explicit file context or body-free file outline | [source-queries.md](references/source-queries.md) |
| Context planning | [context-planning.md](references/context-planning.md) |
| Committed two-revision review | [committed-reviews.md](references/committed-reviews.md) |
| Working-tree, staged, since, or finding-level review | [change-analysis.md](references/change-analysis.md) |
| Quality assessment | [quality.md](references/quality.md) |
| Conditional or compound JSON decision | [decision-queries.md](references/decision-queries.md) |
| Diagnostics and configuration | [diagnostics.md](references/diagnostics.md) |
