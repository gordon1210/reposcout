# Diagnostics and configuration

Use this reference when a scan is partial, slow, crashing, missing expected files or graph edges,
or affected by repository/global configuration.

## Inspect gaps before conclusions

Check top-level diagnostics for unsupported, unreadable, oversized, or omitted files; walker
errors; duration limits; and partial Type-2 analysis. Inspect bounded sample paths before deciding
that an unsupported count is irrelevant. Parse, import, resolver-configuration, and test-mapping
gaps can make an apparent absence uncertain.

For change summaries, distinguish relevant gaps in the observed change neighborhood from
repository-wide blind spots. For graph/context output, retain resolver provenance and do not turn a
heuristic or unresolved edge into certainty. Manual source search, the complete diff, and project
tests remain necessary when gaps intersect the task.

## Diagnose partial Type-2 analysis

When `type2_analysis_partial` is true, inspect skipped candidate buckets, seed pairs, retained
matches, overlap work, and the reported limit reason. Retained near-duplicate groups remain useful,
but absence is not evidence of completeness. The current CLI has no undocumented exhaustive
override; do not invent one or silently rerun with unbounded settings.

## Inspect configuration without changing it

RepoScout has no configuration generator. Configuration is created manually as
`reposcout.toml` or `.reposcout.toml` in a project, or as the OS-specific global
`reposcout.toml`; do not create one merely because no file exists.

```sh
reposcout config <path>
reposcout config -f json <path>
```

Precedence is CLI flags, then the nearest project `reposcout.toml`/`.reposcout.toml`, then the
OS-appropriate global configuration, then defaults. File-backed arrays replace lower-layer arrays;
repeated CLI exclusions and health includes/excludes extend their effective lists.

Health selection order is:

1. `health_scope` establishes `source` or `all`.
2. `health_includes` adds named content formats.
3. `health_excludes` removes repository-relative path globs and wins over includes.
4. Ordinary discovery exclusions remove paths from the entire scan.

Health exclusions retain inventory and navigation facts while removing health signals. Use
`--no-project-config` or `--profile safe` when repository-owned configuration is not trusted.
Never create or change configuration or `.reposcoutignore` until the user approves the exact
proposal and its precedence/effects.

## Diagnose slow or crashing runs

Add a unique debug-log path to the narrowest equivalent reproduction:

```sh
reposcout --debug-log /tmp/reposcout-debug-YYYY-MM-DD.jsonl <path>
```

The path must not exist. Inspect the last `stage_start`, `scan_stage`, or unmatched `file_start`
first. Compare `file_start`/`file_end` durations for outliers. Two-second heartbeats expose the last
meaningful event, quiet duration, and Linux memory; `render_start` without `render_end` points to
serialization rather than analysis.

A `panic` record includes its location and backtrace. Hard termination cannot append a final
event, so retain and interpret the records already flushed.

For Type-2 work, inspect the latest `type2_progress.phase` and completed/total counters before
changing inputs. Final pool events state whether seed-pair, match-buffer, or overlap-suppression
bounds made the result partial. Debug logs contain paths, arguments, configuration, and
backtraces—but not source bodies—so still treat them as potentially sensitive when sharing.
