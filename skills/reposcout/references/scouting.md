# Repository scouting

Use this reference for initial repository orientation, exact zero-config behavior, profile choice,
compact summaries, symbol lookup, and single-file explanation.

## Know the zero-argument contract

With no global or project configuration, options, subcommand, or path, `reposcout` is equivalent
to `reposcout .` with the `full` profile:

- the current path is scanned and the surrounding Git worktree becomes the report root when one
  exists;
- a terminal receives the human-readable table report, while redirected or captured stdout
  receives full JSON;
- hidden files, recognized lockfiles, Git-ignored paths, `.ignore`, and `.reposcoutignore` rules
  are excluded, and symlinks are not followed;
- every recognized text format contributes inventory, tokens, and line facts;
- the source-first health corpus runs complexity, markers, duplication, risk, test presence, and
  cleanup signals, while Git churn and imports are also enabled;
- the OS cache is enabled and no file is written into the scanned repository; and
- context plans, graph/impact queries, review, baselines, output files, updates, and the daemon
  remain off until explicitly requested.

The default health corpus is programming/build source. Content formats enter health analysis only
through configuration or explicit health flags; inventory remains complete.
`summary.assessment.fits_context` uses readable source/build tokens, while `summary.tokens`
remains the complete recognized inventory.

## Choose a profile

Prefer the smallest command that answers the question:

```sh
# Cheap, compact orientation for an agent
reposcout --agent-summary <path>

# Full health, duplication, and churn evidence
reposcout --agent-summary --profile full <path>

# Guardrailed scan of an untrusted checkout
reposcout --agent-summary --profile safe <path>
```

The native view is JSON-only, defaults to the `agent` profile, limits each leading ranking, and
keeps the complete document under 16 KiB. Every bounded list reports entries available, shown, and
omitted from the projection. Coverage gaps, context-plan omissions, unavailable analyzers, and
projection omissions remain distinct.

When the task needs an explicit detail block or one narrower predicate, use ordinary summary JSON
and project it before reading the command result:

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
      source: .summary.source,
      scope: {basis: .work_scope.basis, inventory: .work_scope.inventory}
    }'
```

Use `jq -c` with an explicit selector. Bare `jq` preserves every field and expands compact JSON
with indentation, so it is not an output-budget strategy by itself. In Bash or Zsh, retain
`pipefail` so a failed RepoScout producer cannot be hidden by a successful empty `jq` consumer.

The `agent` profile disables duplication and churn by default. The `safe` profile additionally
ignores project configuration and enforces conservative worker, history, discovery, context, and
duplication settings. Explicit analyzer selection can opt an analyzer back in, but no profile
claims a universal runtime bound for an arbitrarily large target.

Use `--no-project-config` when only the repository-configuration trust boundary is needed. Use
`full` only when the task actually needs the expensive signals it enables.

## Understand compact output

`--agent-summary` retains only common scouting decisions: compact interpretation and coverage,
inventory, assessment, leading quality/navigation signals, and optional direct-versus-expansion
context evidence. It rejects pretty printing and detailed directory, baseline, graph, impact,
review, snippet, or duplicate-pair requests so an explicit answer is never silently discarded.

`-f json --summary` drops heavy `files[]`, raw `duplicates`, and canonical finding arrays while
retaining aggregates and explicitly requested context, directory, graph, impact, baseline, or
review blocks. Context retains ranked file metadata and outline totals but omits declaration
objects; remove `--summary` when signatures are required. Compact duplicate rankings remain
available when duplication ran. Use `--baseline-ready` only when a finding-complete compact
baseline artifact is explicitly required.

In agent-summary, inspect `coverage`, `assessment`, and `interpretation.analyzers` before selecting
files, then use the bounded `signals` rankings. In ordinary summary output, inspect top-level
`diagnostics` and `summary.assessment` first. Never turn a disabled analyzer into a clean zero.

## Use focused queries before broad search

Locate a declaration without a full cross-file analysis:

```sh
reposcout locate <symbol> <path> -f json
```

Start with ranked matching. Add `--exact`, `--kind`, `--language`, or `--limit` only when useful.
An empty result is not proof that a symbol is absent because lookup covers cached declarations for
first-class languages; fall back to ordinary text search.

Explain one file in repository context:

```sh
reposcout explain <file> -f json
```

Use its discovery status, risk inputs, filename-based test matches, dependencies, dependents, and
findings to decide which source to inspect directly. Move to context planning only when a bounded
multi-file reading set is needed.
