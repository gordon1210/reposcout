---
name: reposcout
description: Scout and query software repositories with the RepoScout CLI to plan codebase reading, locate symbols, measure repository health, explain files, map dependencies, assess change impact, and review diffs. Use before exploring an unfamiliar codebase, planning implementation or refactoring work, selecting files for an agent context, investigating declarations or dependencies, reviewing staged, working-tree, or branch changes, or evaluating complexity, duplication, churn, risk, and test presence.
---

# RepoScout

Use RepoScout as an evidence-gathering pass before reading a repository broadly. Prefer compact
JSON results, then open only the files that answer the task. RepoScout reports facts and rankings;
it does not return source bodies or replace direct inspection of the selected code.

## Start safely

1. Verify that `reposcout` is on `PATH`:

   ```sh
   command -v reposcout
   reposcout --version
   ```

   If it is missing, report that the CLI is required and follow the
   [source installation instructions](https://github.com/gordon1210/reposcout#install). Do not
   clone, build, or install software without the user's authorization.

2. Discover the installed contract when the version or available features are uncertain:

   ```sh
   reposcout capabilities -f json
   ```

   This command performs no repository scan. Use its advertised commands, formats, profiles, and
   limits instead of assuming that a locally installed binary has every current feature.

3. Target the narrowest repository path that covers the task. Run only one scan at a time, retain
   the cache, and keep results on stdout unless the user requests an artifact. Never start
   `reposcout daemon` unless the user explicitly asks for a long-running process.

4. Use `safe` for an unfamiliar or untrusted checkout. It ignores repository-owned configuration
   and applies conservative resource guardrails:

   ```sh
   reposcout -f json --summary --profile safe <path>
   ```

## Know the zero-argument contract

With no global or project configuration, options, subcommand, or path, `reposcout` is equivalent to
`reposcout .` with the `full` profile:

- the current path is scanned and the surrounding Git worktree becomes the report root when one
  exists;
- a terminal receives the human-readable table report, while redirected/non-terminal stdout
  receives full JSON;
- hidden files, recognized lockfiles, Git-ignored paths, `.ignore`, and `.reposcoutignore` rules
  are excluded; symlinks are not followed;
- every recognized text format contributes inventory, tokens, and line facts;
- the source-first health corpus runs complexity, markers, duplication, risk, test-presence, and
  cleanup signals, while Git churn and imports are also enabled;
- the OS cache is enabled and no file is written into the scanned repository; and
- context plans, graph/impact queries, review, baselines, output files, updates, and the daemon
  remain off until explicitly requested.

The default health corpus is programming/build source. Optional content formats enter only through
`--health-include` / `health_includes` or all-content scope. Repository-relative
`--health-exclude` / `health_excludes` globs are applied last and win over includes; they retain
inventory/navigation facts while removing health signals. `summary.assessment.fits_context` uses
`summary.source.tokens`, while `summary.tokens` remains the complete recognized inventory.
Filename-based test matching is navigation evidence only and does not change risk or cleanup
scoring. Check bounded `diagnostics.unsupported_samples` before assuming an unsupported count is
irrelevant.

## Respect configuration authority

RepoScout has no configuration generator. A configuration is created manually as
`reposcout.toml` or `.reposcout.toml` in a project, or as the OS-specific global
`reposcout.toml`. Inspect the current resolution before proposing a change:

```sh
reposcout config <path>
reposcout config -f json <path>
```

Precedence is CLI > nearest project file > global file > defaults. File-backed arrays replace the
lower layer; repeated CLI `--exclude`, `--health-include`, and `--health-exclude` values extend the
resolved lists. Health selection order is:

1. `health_scope` establishes `source` or `all`;
2. `health_includes` adds named formats such as `json` or `markdown`;
3. `health_excludes` removes matching repository-relative path globs; and
4. ordinary discovery exclusions remove paths from the entire scan.

Never create or modify a global/project RepoScout configuration or `.reposcoutignore` without the
user's explicit approval. Explain the exact proposed keys, affected paths/analyzers, precedence,
and inventory-versus-health consequence first. Reading an existing configuration and using
`reposcout config` are non-mutating; use `--no-project-config` or the `safe` profile when
repository-owned configuration is not trusted.

## Diagnose slow or crashing runs

When a scan stalls, crashes, or consumes unexpected resources, add a unique debug-log path to the
narrowest reproducing command. Keep the same profile and analyzer options so the reproduction
remains equivalent:

```sh
reposcout --debug-log /tmp/reposcout-debug-2026-07-29.jsonl <path>
```

The path must not exist. Inspect the last `stage_start`, `scan_stage`, or unmatched `file_start`
record first; compare paired `file_start` / `file_end` durations to find outliers. A `heartbeat`
arrives every two seconds during otherwise quiet work and includes the last meaningful event,
quiet duration, and Linux resident/peak memory. `render_start` without `render_end` points to
serialization rather than analysis.

For Type-2 duplication, inspect the latest `type2_progress` record. Its `phase` narrows the work to
indexing, candidate planning/search, sorting, overlap suppression, or materialization. Compare the
completed/total counters and throughput across records; candidate search also exposes the current
bucket size, total versus admitted seed work, verification-token comparisons, and retained matches.
The `pool_finished` / `finished` records say whether Type-2 analysis became partial because a
seed-pair, match-buffer, or overlap-suppression limit was reached. A Rust `panic` record includes
its location and backtrace. Hard termination cannot add a final event, so retain the earlier
flushed records. Debug logs contain paths, arguments, configuration, and backtraces but no source
contents; treat them as potentially sensitive when sharing.

The current CLI has no high-budget or exhaustive Type-2 override. Do not invent one or rerun with
undocumented flags; report the partial result honestly. An explicit effort mode is tracked in the
project roadmap.

## Scout a task

Run a compact initial scan for ordinary codebase orientation:

```sh
reposcout -f json --summary --profile agent <path>
```

The `agent` profile omits duplication and churn by default. Use `full` only when those signals or
a complete cleanup assessment matter:

```sh
reposcout -f json --summary --profile full <path>
```

Interpret the response in this order:

1. Check top-level `diagnostics` for unsupported or unreadable files, walker errors, and
   `type2_analysis_partial`. When Type-2 is partial, treat near-duplicate findings and combined
   duplication percentages as lower bounds; use the skipped-work and limit-reason fields to see
   why.
2. Check `summary.assessment`, including its completeness fields and `unavailable_signals`; never
   interpret a disabled analyzer as a measured zero.
3. Use `summary.top_risks`, `summary.complexity_violations`, `summary.test_presence`,
   `summary.skip_candidates`, `summary.symbols`, and `summary.top_token_files` to choose where to
   investigate.
4. When full analysis is enabled, inspect `summary.duplication`, `summary.top_duplicates`, and
   `summary.top_hotspots` as prioritization signals rather than automatic refactoring decisions.
   Raw duplication covers the configured health corpus, including tests; the cleanup assessment
   separately uses production code and excludes direct Rust `#[cfg(test)]` regions.

Request a bounded reading plan when the task needs source inspection:

```sh
reposcout -f json --summary --profile agent \
  --focus <file-or-directory> --context-budget 24000 --context-max-files 15 <scan-root>
```

`--focus` enables the context plan. Follow `context.files` in rank order, use its selection reasons
and declaration outlines, and note `context.omitted`. Open the chosen source with normal repository
tools before proposing or making changes.

## Use focused queries

Locate a declaration before searching the whole tree manually:

```sh
reposcout locate <symbol> <path> -f json
```

Start with ranked matching. Add `--exact`, `--kind <kind>`, `--language <language>`, or
`--limit <1..100>` only when useful. An empty result is not proof that a symbol is absent: lookup
uses cached declaration outlines for first-class languages, so fall back to ordinary text search.

Explain why one file matters in its surrounding repository:

```sh
reposcout explain <file> -f json
```

Use the result to inspect discovery status, ignore rules, risk factors, matching tests,
dependencies, dependents, and findings.

Build a bounded dependency neighborhood only when relationships are relevant:

```sh
reposcout -f json --summary --graph-focus <file> \
  --graph-direction both --graph-depth 2 <scan-root>
```

Treat graph resolution as conservative evidence. Report unresolved imports, parse/configuration
errors, and confidence limitations instead of presenting every inferred edge as certain.

## Assess changes

Choose exactly one diff scope:

- `--working` for uncommitted worktree changes.
- `--staged` for index changes.
- `--since <ref>` for changes since a branch, tag, or commit.

For a reading plan and blast radius while keeping scan metrics change-scoped, run:

```sh
reposcout --working --change-summary -f json <path>
```

Substitute the appropriate diff scope. `--change-summary` defaults to the `agent` profile, implies
context and impact analysis, and emits only the bounded decision projection. Confirm that
`capabilities.change_summary.flag` exists before using it with an older installed binary; otherwise
fall back to:

```sh
reposcout -f json --summary --profile agent \
  --working --context --impact <path>
```

Interpret the concise response in this order:

1. Read `change_summary.coverage`: `observed_scope_confidence` describes the known change
   neighborhood, while `discovery_completeness` separately records repository-wide blind spots.
2. Check `relevant_gaps` before `outside_known_scope_gaps`; distant gaps can hide an edge but are
   not mislabeled as failures in the observed scope.
3. Follow `reading_order`, then inspect bounded matching tests and impact entries. Matching tests
   remain convention-based evidence, not measured coverage.
4. Check every `omitted` counter. Capability discovery advertises the aggregate path, gap, and
   validation limits.

Use `--profile safe` explicitly for an untrusted checkout or `--profile full` only when the
change decision also needs the full analyzer set. Request the existing detailed
`--summary --context --impact` workflow when declaration outlines or complete context/impact
blocks are actually needed.

For finding-level review, enable the full analyzers and compare both snapshots when the task
warrants the additional work:

```sh
reposcout -f json --summary --profile full \
  --since <ref> --review=deep <path>
```

Report `new`, `worsened`, `resolved`, and `improved` findings separately. Do not add failure gates,
write baselines, or create SARIF/output files unless the user asked for CI or persistent artifacts.

## Report conclusions

State the scanned target, profile, and diff scope. Lead with coverage gaps, then the few findings
that affect the task, the recommended reading order, and any likely test or dependent files.
Distinguish measured facts from heuristics: matching tests are not measured coverage, risk is a
ranking signal, and import/type resolution can be partial.
