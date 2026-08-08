# Agent repository map

This is a normative extension of the root [`AGENTS.md`](../../AGENTS.md). Read it completely before
choosing implementation files, changing module boundaries, or touching frontend structure. The
root instructions remain in force.

## Layout and ownership

```text
CHANGELOG.md          User-visible changes in reverse chronological order.
.agents/skills/reposcout/
                      Deterministic repository-local mirror of the bundled agent skill.
apps/
  web/                React dashboard for the live daemon.
  landing/            Bespoke public RepoScout landing page.
packages/
  eslint-config/      Shared flat ESLint configuration for both frontend apps.
scripts/
  reposcout-skill.sh  Synchronizes and validates the bundled skill mirror.
skills/
  reposcout/          Canonical distributable agent skill and focused references.
src/
  main.rs            CLI entry: scan/query/explain dispatch, profiles, gates, output/errors,
                     and debug-session lifecycle.
  lib.rs             Re-exports modules so tests and the binary share one crate.
  cli.rs             Clap definitions: Cli, Command, ScanArgs, and OutputFormat.
  config.rs          Layered defaults/global/project config resolution and inspection.
  debug_log.rs       Flushed NDJSON run diagnostics, quiet-work heartbeat, process-memory
                     sampling, and panic capture.
  context.rs         Deterministic token-budgeted, focus/change-aware reading-plan ranking and
                     bounded structural-outline projection.
  work_scope.rs      Bounded observed repository, seed, context, impact, component, and
                     confidence facts for agent decisions.
  model.rs           Shared stable serializable data contract.
  lang.rs            Language detection by extension and filename.
  walk.rs            Ignore-aware discovery, Git root, custom .reposcoutignore, stable
                     report/cache paths, and walker-error accounting.
  scan.rs            Orchestrator for per-file analysis, cache, aggregation, duplication,
                     findings, rollups, baseline/review, diff scopes, diagnostics, and impact.
  findings.rs        Canonical complexity/marker/duplication/risk findings, fingerprints,
                     comparison, and rename remapping.
  review.rs          Changed-line filtering and deep Git-snapshot comparison.
  snapshot.rs        Worktree/tree/index adapters and shared snapshot policy.
  explain.rs         Repository scan projected onto one requested file.
  query.rs           Capabilities and bounded symbol lookup.
  cache.rs           OS-cache-backed incremental facts, manual reset, profile validation, and
                     ANALYZER_VERSION invalidation.
  parse/mod.rs       Tree-sitter Language and parse() for first-class languages.
  php.rs             PHP namespace-use and static-include normalization.
  graph.rs           Opt-in structural graph, resolver provenance, package/module metadata,
                     focus queries, cycles, orphans, virtual deletions, and reverse impact.
  graph/symbols.rs   Conservative syntax-proven cross-file type relations.
  metrics/
    tokens.rs        tiktoken counting with o200k_base or cl100k_base.
    lines.rs         LOC, SLOC, comment, and blank-line facts.
    markers.rs       Comment-aware TODO/FIXME/HACK analysis.
    complexity.rs    Cyclomatic, cognitive, nesting, Halstead, and MI metrics.
    imports.rs       Root-module import/dependency extraction.
    symbols.rs       Symbol counts and compact declaration headers.
    classify.rs      Generated/minified/bundled/vendored skip hints.
    testcov.rs       Test/source classification, filename/framework matching, and Rust
                     inline-test detection.
    risk.rs          Shared composite risk calculation and explain factors.
  dup/
    mod.rs           Prepared-corpus orchestration, format pools, cleanup, findings, and union
                     line/token coverage.
    tokenize.rs      Structured AST/fallback lexer with precise ranges.
    exact.rs         Format-scoped rolling-hash Type-1 detection.
    fuzzy.rs         Type-2 fingerprints, identifier bijection, bounded verification,
                     diagonal coverage, overlap suppression, and progress.
    fuzzy/plan.rs    Deterministic rare-first admission and work accounting.
  git.rs             Churn, changed-file/line diffs, and rename detection.
  report/            JSON, table, Markdown, SARIF, NDJSON, DOT/Mermaid, config, explain, and
                     query renderers.
tests/
  cli.rs             End-to-end integration tests against bounded fixtures.
  dup_languages.rs   Detector and CLI matrix for every canonical format.
  support/           Shared fixture and CLI-command helpers.
  fixtures/dup_languages.toml
                     Exact and Type-2 samples for all 31 formats.
  fixtures/sample/   Small multi-language fixture tree.
```

## Frontend boundaries

`apps/web/src/components/ui/` contains imported shadcn primitives. It is globally ignored by
ESLint and must not be edited by hand.

Frontend production code has a cyclomatic-complexity ceiling of 20 and a 900-line module ceiling.
Tests retain correctness and formatting checks but are exempt from size, complexity, strict
assertion, and development-only React rules.

Keep the dashboard runtime shell in `apps/web/src/components/dashboard.tsx` and report rendering in
`dashboard-report.tsx`. Repository-graph responsibilities are deliberately split across the
controller, workspace view, canvas renderers, layout decoration, detail panels, and pure helpers
under `apps/web/src/components/repository-graph-*` and `apps/web/src/lib/graph-*`. Preserve those
seams rather than rebuilding a monolithic dashboard or graph component.
