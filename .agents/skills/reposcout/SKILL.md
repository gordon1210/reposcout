---
name: reposcout
description: Scout and query software repositories with the RepoScout CLI to plan codebase reading, locate symbols, measure repository health, explain files, map dependencies, assess change impact, and review diffs. Use before exploring an unfamiliar codebase, planning implementation or refactoring work, selecting files for an agent context, investigating declarations or dependencies, reviewing staged, working-tree, or branch changes, or evaluating complexity, duplication, churn, risk, and test presence.
---

# RepoScout

Use RepoScout as an evidence-gathering pass before reading a repository broadly. It reports facts,
rankings, bounds, and confidence gaps; it does not return source bodies or replace direct source
inspection. The coding agent—not RepoScout—decides what to read, whether to delegate or split work,
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

For compact agent scouting, start with:

```sh
reposcout -f json --summary --profile agent <path>
```

When only specific decisions are needed, discard unrelated fields before stdout enters model
context. Use a targeted compact projection, not bare `jq` (which only pretty-prints the full
payload), and do not retain bounded sample arrays unless they answer the task. In Bash or Zsh,
preserve the producer's failure status with `pipefail`:

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

## Load focused guidance

Read only the references required by the current task. Load more than one only when the task spans
those workflows.

| Representative task | Read |
|---|---|
| Repository scouting | [scouting.md](references/scouting.md) |
| Context planning | [context-planning.md](references/context-planning.md) |
| Change analysis | [change-analysis.md](references/change-analysis.md) |
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

Distinguish measured facts from heuristics: filename matching is not measured coverage, risk is a
versioned ranking, graph resolution can be partial, and compact duplicate lists are projections
over retained raw findings. Do not present graph components or numeric thresholds as proof that
work should be delegated or split.

## If RepoScout is unavailable

Do not spend calls on routine `command -v`, `--version`, or capability preflights. Run the selected
task command directly. If the shell reports that `reposcout` is not installed, tell the user and
stop; do not clone, build, or install it without explicit authorization.
