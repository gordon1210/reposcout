---
name: reposcout
description: "Use for any question about a change between two commits, branches or a pull request: reviewing it, judging whether it is safe to merge or ship, explaining what changed, or finding defects, regressions and risks. Run `reposcout review-context . --base <base> --head <head> --profile safe --brief` before `git diff` or `git log`: one call returns the changed hunks, changed definitions, importers, call sites and docs that name them. Also use it to explore an unfamiliar codebase, plan implementation or refactoring work, or choose files to read: run `reposcout --agent-summary .` before `ls`, `find` or `grep` sweeps; one compact call returns the file inventory, largest and riskiest files, symbols, complexity and test presence. It also locates symbols, reads known definitions and maps dependencies and change impact."
---

# RepoScout

Use RepoScout to gather evidence before reading a repository broadly, or to read an explicitly
known definition under a shared output budget. Ordinary scouting and context reports retain
body-free facts, rankings and confidence gaps. The coding agent decides which source is needed
and which validations the task requires.

## Start safely

- Target the narrowest path that covers the task. Run one scan at a time, retain the cache, and
  keep results on stdout unless the user requests an artifact.
- When several related focus paths are already known, pass every path to one context-planning
  invocation with repeated `--focus` flags. They then share one token budget and file limit;
  do not request a separate full-budget plan for each known focus.
- Use `--profile safe` for an unfamiliar or untrusted checkout. It ignores repository-owned
  configuration and applies conservative resource guardrails.

Never start `reposcout daemon`, a frontend, or another long-running process unless the user
explicitly authorizes it.

## Choose the smallest start

On Unix, if a file and symbol or line are already known and its source is needed, use `reposcout read`
directly; load the focused source-query guidance below. If the exact complete small span is
already known and no snapshot, hash or coverage evidence is needed, use a targeted native read.
Do not repeat unchanged source already available in context. Optional `--outline` is not a required
first step. When the entry point is unknown, use lexical `find` only if its declaration candidates
answer the next question. Known definitions can share a `plan`; known error logs can seed context.
Use the matching focused guidance below and preserve snapshot/hash identity on follow-up reads.
When module imports or registrations are essential, use explicit `read --range FILE START END`
for known small spans, or `read --file` when complete file context is needed. Keep expected hashes
and snapshot identity. A bounded preamble is caller-selected evidence, not proof of complete import
coverage. Prefer smaller definition reads when sufficient; do not repeat retained unchanged source.

For a working-tree, staged or since-ref comparison, start with bounded `reposcout changes` when
the next decision needs changed definitions; request `--source` only for bodies needed by that
decision. The change-analysis guidance describes this route and optional broader report
integration. For two committed revisions (a pull request, branch or commit range), follow
[Review two commits](#review-two-commits-pr-or-branch-review) below. Preserve configured
tokenization and leave agent allocation and task splitting entirely to the caller.

For compact agent scouting, start with:

```sh
reposcout --agent-summary <path>
```

This JSON-only view defaults to the `agent` profile, caps each ranking and context tier, and keeps
the complete document under 16 KiB. Read coverage and omission counters before treating absence as
evidence. For a focused or change-seeded plan, first check `context.outline_only.entries` for
explicit seeds that exceed the body budget, then read `context.direct_evidence`; inspect
`expand_if_needed` only when the direct files do not answer the next decision. An unseeded context
plan has no direct evidence by design; its expansion entries are only a bounded orientation
shortlist.

Use ordinary `-f json --summary` only when the task needs a detailed graph, impact, review,
baseline, directory block, or a predicate the native view does not expose. Then discard unrelated
fields before stdout enters model context. Use a targeted compact projection, not bare `jq` (which
only pretty-prints the full payload), and preserve the producer's status with `pipefail`:

```sh
set -o pipefail

reposcout -f json --summary --profile agent <path> \
  | jq -c '{
      coverage: (.diagnostics | {
        discovered_files, analyzed_files, unsupported_files,
        unreadable_files, walker_errors,
        scan_truncated: (.scan_truncated // false)
      }),
      assessment: .summary.assessment,
      scope: {basis: .work_scope.basis, inventory: .work_scope.inventory}
    }'
```

Use a bare `reposcout` only when the exact zero-argument/default behavior is requested. A terminal
receives a human table, but captured or redirected stdout receives full JSON; read the repository
scouting guidance before interpreting that default.

## Review two commits (PR or branch review)

1. Run once:

   ```sh
   reposcout review-context . --base <base> --head <head> --profile safe --brief
   ```

   It pins both commits and prints, within 4k tokens by default: recurring token rewrites
   (`R1 old → new`), moved and renamed code with destinations, old spellings still in head, and the
   changed hunks with their changed definitions, direct importers and call/reference sites
   (`path:line`), docs naming changed definitions, nonzero gaps, and `unresolved dispatch` for
   added implementations that no caller resolves. Hunks that only apply pure identifier renames
   are listed as sites; hunks tagged `[R1 R2]` show only their changed lines because every edit is
   a listed rewrite. Moved code keeps its text; check that the names it uses resolve the same at
   the destination. Read hunks marked `deviates from Rn` first: they break a
   recurring edit. Judge whether each rewrite belongs at its sites instead of fetching them. Run
   `git diff` only for omitted hunks, with the commands the brief prints.
2. Read what the brief does not show, as line ranges at the pinned commit, several ranges per
   command: `git show <head>:path | sed -n '40,90p'`, or `reposcout read . --snapshot <head>
   --range FILE START END -f json`. Avoid dumping whole large files.
3. Consumers and doc mentions are starting points, not a complete boundary. When gaps are nonzero
   or dispatch is dynamic (registries, string keys, config), search the pinned commit with
   `git grep -n PATTERN <head> -- DIR`. When a change adds variants to an existing registry,
   interface, enum or config, follow each input a new variant requires, or value it emits, into
   the unchanged code that validates, stores or orders it: one `git grep` for that key or field
   outside the changed files, then read the hits. A listed consumer is a lead, not evidence: before
   concluding that nothing breaks, read each listed call site or usage line in context, or run the
   tests that exercise it, and cite what you checked.
4. RepoScout replaces lookups, not verification. Run the repository's documented check or test
   command (README, contributing notes, package scripts) where the task permits; a no-issues
   verdict needs at least one executed check. A green suite does not cover new variants: give each
   finding, or else the riskiest traced path, one minimal witness through the public entry point,
   repeating the operation when state carries over. A RepoScout call is never an executed check.
5. Cite evidence as path plus line range at the pinned commit, quoting source lines verbatim
   without diff markers. For each finding cite the documented rule or base behavior it breaks,
   the head line that breaks it, and one affected caller on the active path; for
   behavior-preserving claims cite both the base and the head lines.
   Keep static reasoning, executed checks and unexecuted suggestions separate.

## Load focused guidance

Read only the references required by the current task. Load more than one only when the task spans
those workflows.

| Representative task | Read |
|---|---|
| Repository scouting | [scouting.md](references/scouting.md) |
| Known definition, explicit file context or body-free file outline | [source-queries.md](references/source-queries.md) |
| Context planning | [context-planning.md](references/context-planning.md) |
| Committed two-revision review | [committed-reviews.md](references/committed-reviews.md) |
| Working-tree, staged, since, or finding-level review | [change-analysis.md](references/change-analysis.md) |
| Quality assessment | [quality.md](references/quality.md) |
| Conditional or compound JSON decision | [decision-queries.md](references/decision-queries.md) |
| Diagnostics and configuration | [diagnostics.md](references/diagnostics.md) |

## Require configuration consent

Never create or modify a global/project `reposcout.toml`, `.reposcout.toml`, or
`.reposcoutignore` without explicit user approval. Before requesting approval, explain the exact
keys or patterns, affected paths/analyzers, precedence, and inventory-versus-health consequence.
Reading existing configuration and running `reposcout config` are non-mutating.

## Report conclusions

State the scanned target, profile, and diff scope. Lead with relevant coverage gaps, then report
only the findings, reading order, tests, and dependents that affect the task. Include totals,
omissions, and unavailable signals; never turn a disabled analyzer into a measured zero.

Distinguish measured facts from heuristics: runner-backed test-file selection is not measured
coverage, risk is a versioned ranking, graph resolution can be partial, and compact duplicate lists
are projections over retained raw findings. Do not present graph components or numeric thresholds
as proof that work should be delegated or split.

## Trust the CLI boundary

Use only an already-installed `reposcout` binary whose provenance the user trusts. The official
source is [gordon1210/reposcout](https://github.com/gordon1210/reposcout), and official binaries
come from [GitHub Releases](https://github.com/gordon1210/reposcout/releases). If provenance is
uncertain, stop and direct the user to the
[verified installation flow](https://github.com/gordon1210/reposcout/blob/main/docs/getting-started.md#verified-installation).
Never clone, build, install, or update the CLI without explicit authorization.

Do not spend calls on routine `command -v`, `--version`, or capability preflights. Run the selected
task command directly. If the shell reports that `reposcout` is not installed, tell the user and
continue without it.
