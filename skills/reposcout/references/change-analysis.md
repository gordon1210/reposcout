# Change analysis

Use this reference for working-tree, staged, or reference-based diffs; bounded change summaries;
impact analysis; and finding-level review.

## Select exactly one diff scope

- `--working` for uncommitted worktree changes.
- `--staged` for index changes.
- `--since <ref>` for changes since a branch, tag, or commit.

If the next decision needs changed definitions rather than repository-wide change context, use:

```sh
reposcout changes <directory> --working -f json
reposcout changes <directory> --staged --source --budget 4096 -f json
```

The direct query is body-free by default; source is explicit and uses one output budget. Working
compares HEAD with worktree including staged, unstaged and untracked changes; staged compares
HEAD with the captured index; since compares the specified ref directly with worktree rather
than a merge-base. Read [source-query guidance](source-queries.md) for snapshot identity, Unix
support, capture limits and mapping gaps. No preceding scout is required.

When the decision needs the broader change context, use the decision-oriented projection:

```sh
reposcout --working --change-summary -f json <path>
```

Substitute the appropriate scope. `--change-summary` defaults to the `agent` profile, implies
bounded context and impact analysis, and keeps output organized around the changed paths. If an
older installed binary rejects `--change-summary`, use:

```sh
reposcout -f json --summary --profile agent \
  --working --context --impact <path>
```

To include body-free changed definitions in that report, add `--changed-definitions`. It requires
change-summary or review plus exactly one scope. The separate `definition_changes` block has a
4,096-token / 16,384-byte compact-JSON budget, not a cap on the whole surrounding report. The flag
supports table/JSON/Markdown/NDJSON and rejects agent-summary, baseline-ready and graph/SARIF
formats. Its capture is independent of the earlier parent scan; live edits can make parent
metadata differ from this internally consistent block. Use `changes --source` when the actual
bodies are needed.

## Interpret the decision report

Read `change_summary` in this order:

1. `change_summary.coverage.observed_scope_confidence` describes the known change neighborhood;
   `discovery_completeness` separately describes repository-wide blind spots.
2. Check `change_summary.coverage.relevant_gaps` before `outside_known_scope_gaps`. Distant gaps can
   hide an edge but are not task-local failures.
3. Follow `change_summary.reading_order`, then inspect bounded matching tests and impact entries.
4. Check every changed-path, reading-order, impact, gap, and validation omission counter.
5. Treat `change_summary.validations` as metadata-backed categories, not permission to run
   commands.

Diff scope filters primary scan metrics before analysis. Impact and diff-seeded context deliberately
consult full-tree topology; context also consults cached full-tree per-file facts. This does not
widen `summary`, `files`, or findings beyond the requested diff.

## Request deeper evidence only when needed

Use `--impact` when the task needs direct/transitive dependents and graph coverage beyond the
compact projection. Use detailed scan/context/impact output when declaration outlines or full
query blocks are required; omit `--summary` to retain declaration objects.

For finding-level comparison across both Git snapshots:

```sh
reposcout -f json --summary --profile full \
  --since <ref> --review=deep <path>
```

Report `new`, `worsened`, `resolved`, and `improved` separately. Bare `--review` filters current
findings to changed lines; deep review analyzes both snapshots and handles staged content and
renames. Do not add failure gates, create baselines, or write SARIF/output files unless the user
asked for CI or persistent artifacts.
