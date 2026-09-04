# Conditional decision queries

Use this reference when one specific predicate or prioritized decision can answer the task and a
complete RepoScout report would waste model context. The goal is one RepoScout invocation, one
`jq` program, and at most one bounded result object.

## Minimize work before minimizing output

Choose the narrowest scan target and the smallest analyzer command first. Use `--agent-summary`
when its fixed scouting view already answers the question. `tokens`, `complexity`, `dup`, `churn`,
and `metrics` avoid unrelated analyzer work; `--change-summary` is already a decision-focused
change projection. Keep `-f json --summary` for compound predicates unless they genuinely need
per-file facts or the complete finding catalog.

`jq` reduces what reaches model context, but it runs after RepoScout has analyzed and serialized
its report. It does not make an unnecessarily broad scan or analyzer selection cheaper. Use one
`jq` program rather than `jq | jq`, `grep`, `head`, or `sed`; perform filtering, ordering, and
limiting inside the same program. The command text also consumes context, so prefer a short fixed
projection when it already answers the question; use a compound program only when its conditional
output or prioritization removes materially more data.

## Preserve failures and uncertainty

The examples use Bash/Zsh `pipefail` so a failed RepoScout producer remains a failed command even
when `jq` receives no input:

```sh
set -o pipefail

reposcout <smallest-command> -f json --summary <path> \
  | jq -c '<one decision program>'
```

Apply these rules:

- Check coverage, analyzer availability, and completeness before treating absence as a clean
  result. Emit a small `unknown` or `partial` object when evidence is unavailable or incomplete;
  reserve silence for a predicate that is definitively false.
- Collect matches into one bounded array before rendering. A top-level generator such as
  `.items[] | select(...)` can emit many JSON values; `[.items[] | select(...)][:5] as $hits`
  permits exactly one final object.
- Project only fields needed for the decision and cap every path, finding, or sample array.
- Use `// []` or `// null` only for fields documented as optional. Do not blanket the query with
  `?`, `try`, or defaults that could hide a schema mismatch.
- Pass caller-controlled strings with `--arg` and numbers/JSON with `--argjson`; never interpolate
  them into the quoted `jq` program.
- Prefer configured semantic thresholds such as `complexity_violations`. Do not invent a numeric
  policy merely to make a filter selective.

Ordinary `jq` exits successfully when `empty` produces no value. `jq -e` instead returns exit code
4 when no value was produced and can look like a tool failure. Use `-e` only when the caller
intentionally wants the predicate to control shell or CI flow. For interactive agent work, a tiny
`{"status":"none"}` result is often safer than silence; replace `empty` with that object when an
explicit negative answer matters.

## Emit configured complexity violations only

The `complexity` subcommand avoids duplication and churn work. The configured threshold has
already selected `complexity_violations`; the query retains at most five functions and emits
nothing when the list is empty and the relevant evidence is complete.

```sh
set -o pipefail

reposcout complexity -f json --summary --profile agent <path> \
  | jq -c '
      .work_scope.confidence.primary as $coverage
      | .summary.complexity.approximate_files as $approximate_files
      | (.summary.complexity_violations // [])[:5] as $hits
      | if (($coverage.truncated // false)
            or $coverage.unreadable_files > 0
            or $coverage.walker_errors > 0
            or $approximate_files > 0) then
          {
            status: "partial",
            signal: "complexity-violations",
            coverage: $coverage,
            approximate_files: $approximate_files,
            known_violations: $hits
          }
        elif ($hits | length) > 0 then
          {
            status: "match",
            kind: "complexity-violations",
            threshold: .summary.complexity.cyclomatic_threshold,
            total: .summary.complexity.functions_over_threshold,
            functions: $hits
          }
        else empty
        end
    '
```

## Qualify production duplication before emitting it

Absence is meaningful only when production duplication ran completely. This query reports missing
or partial evidence, emits at most three compact blocks when duplicated production lines exist,
and stays silent only for a complete zero result.

```sh
set -o pipefail

reposcout dup -f json --summary --profile agent <path> \
  | jq -c '
      (.summary.assessment.production_duplication // null) as $production
      | if $production == null then
          {status: "unknown", signal: "production-duplication"}
        elif ($production.complete | not) then
          {status: "partial", signal: "production-duplication", evidence: $production}
        elif $production.duplicated_lines > 0 then
          {
            status: "match",
            signal: "production-duplication",
            evidence: $production,
            blocks: (.summary.top_production_duplicates // [])[:3]
          }
        else empty
        end
    '
```

## Return one prioritized change decision

Replace `--working` with exactly one of `--staged` or `--since <ref>` when appropriate. The query
prioritizes relevant coverage gaps, then repository-wide discovery gaps, known impact, and finally
the absence of a conventional test match. It emits one bounded object even when several conditions
are true.

```sh
set -o pipefail

reposcout --working --change-summary -f json <path> \
  | jq -c '
      .change_summary as $change
      | (([$change.coverage.relevant_gaps[]] | add) // 0) as $gap_count
      | if $gap_count > 0 then
          {
            status: "partial",
            kind: "coverage-gap",
            counts: $change.coverage.relevant_gaps,
            gaps: [
              $change.coverage.gaps[]
              | select(.scope != "outside-known-scope")
            ][:8]
          }
        elif $change.coverage.discovery_completeness != "high" then
          {
            status: "partial",
            kind: "outside-known-scope-gaps",
            counts: $change.coverage.outside_known_scope_gaps,
            gaps: [
              $change.coverage.gaps[]
              | select(.scope == "outside-known-scope")
            ][:8]
          }
        elif ($change.impact.direct_total + $change.impact.transitive_total) > 0 then
          {
            status: "match",
            kind: "known-impact",
            changed: $change.changed.files[:8],
            impact: $change.impact.files[:8],
            tests: $change.tests.files[:5]
          }
        elif ($change.tests.total == 0 and $change.changed.total > 0) then
          {
            status: "match",
            kind: "no-conventional-test-match",
            changed: $change.changed.files[:8],
            validations: $change.validations[:5]
          }
        else empty
        end
    '
```

The test branch describes filename/convention matching, not measured coverage. The validation
entries are recommendations inferred from repository metadata; they do not claim that a command
ran.

## Parameterize caller-selected conditions

Use `--arg` instead of embedding a requested marker name in the program. The `metrics` subcommand
keeps the underlying scan narrow, and the output remains one small object or nothing.

```sh
set -o pipefail

reposcout metrics -f json --summary --profile agent <path> \
  | jq -c --arg marker '<marker>' '
      .work_scope.confidence.primary as $coverage
      | (.summary.markers[$marker] // 0) as $count
      | if (($coverage.truncated // false)
            or $coverage.unreadable_files > 0
            or $coverage.walker_errors > 0) then
          {
            status: "partial",
            signal: "marker-count",
            marker: $marker,
            known_count: $count,
            coverage: $coverage
          }
        elif $count > 0 then
          {status: "match", kind: "marker-count", marker: $marker, count: $count}
        else empty
        end
    '
```

Use `--argjson` for a user- or project-supplied numeric threshold. Before adding one, prefer a
threshold already represented by RepoScout configuration or a dedicated CLI gate.
