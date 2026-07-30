# Agent workflows

← [Documentation index](README.md)

RepoScout is a local, deterministic scouting layer. It helps an agent decide what to read, skip,
or inspect next without copying source into the report or calling a model.

## Start with a compact scout

```sh
reposcout -f json --summary --profile agent <PATH>
```

`--summary` retains aggregate and top-N evidence while removing heavy per-file, duplicate-family,
and canonical-finding arrays. Explicit context, directory, graph, impact, baseline, and review
blocks remain when requested.

The most useful first-pass fields are:

| Question | Evidence |
|---|---|
| Will readable source fit? | `summary.source.tokens`, `summary.assessment` |
| How large is the complete inventory? | `summary.tokens`, `summary.files` |
| Is cleanup worthwhile? | duplication, complexity violations, risks, and assessment reasons |
| What should be read first? | `summary.top_risks`, `summary.top_source_token_files` |
| What can be skipped? | `summary.skip_candidates` and each reason |
| Are tests present? | `summary.test_presence` |
| Did the scan cover the target? | top-level `diagnostics` |
| Was Type-2 analysis complete? | `diagnostics.type2_analysis_partial` |

The serialized `untested_*` fields mean “no conventional matching test file or inline Rust test
was found.” Rust `tests/cli.rs` also conventionally matches `src/main.rs`; these are
test-presence heuristics, not measured coverage, and they do not change risk or cleanup scoring.

## Discover capabilities

Do not hard-code assumptions about an installed binary:

```sh
reposcout capabilities -f json
```

This command performs no repository scan. It advertises commands, output/error formats, profiles,
language coverage, health scopes and path-exclusion support, symbol kinds, and hard graph,
duplication, and change-summary bounds.

## Find declarations

```sh
reposcout locate HttpClient . -f json
reposcout locate App\\Service\\Mailer . --exact --kind class -f json
```

`locate` searches cached declaration outlines across first-class languages. Results include the
path, line, language, declaration kind, qualified name, signature, export/public status, and
stable relevance rank. Use:

- `--exact` for case-sensitive qualified or simple-name equality;
- `--kind <KIND>` and `--language <LANGUAGE>` to filter; and
- `--limit <1..100>` to cap results.

A cold query performs configured per-file analysis to populate the ordinary scan cache. It does
not run whole-corpus duplication or Git churn.

## Build a bounded reading plan

```sh
reposcout -f json --summary --context \
  --context-budget 24000 --context-max-files 15 .
```

The top-level `context` block ranks already analyzed files under both hard limits. Reasons may
include:

- explicit focus or changed-file membership;
- matching tests;
- supported import/type-relationship neighbors;
- repository instructions, manifests, and entry points;
- nearby risky code, churn, and complexity; and
- useful same-directory context.

Selected first-class-language files carry bounded, body-free declaration outlines. RepoScout
never embeds source bodies in the plan and never exceeds the requested aggregate source-token or
file budget.

Focus one or more paths:

```sh
reposcout -f json --summary \
  --focus src/service.ts --focus tests/service.test.ts .
```

An oversized explicit focus may appear as `outline_only`: its source does not fit the budget, but
its bounded declarations remain useful. Unmatched focus values stay explicit rather than becoming
invented seeds.

## Plan from a change

```sh
reposcout --working --change-summary -f json .
reposcout --since main --change-summary -f json src/
```

`--change-summary` requires exactly one diff scope, defaults to the `agent` profile, and implies
compact rendering, context planning, and impact analysis. The primary scan remains diff-scoped;
the shared full-tree planning/topology universe supplies unchanged tests, dependencies, and
dependents without adding general health rankings to the result.

Read the dedicated projection in this order:

1. `coverage.observed_scope_confidence` — whether eligible changed files and the known impact
   neighborhood have clean graph evidence;
2. `coverage.discovery_completeness` — whether repository-wide parse, resolution, configuration,
   unreadable-file, or scan-limit gaps could hide more relationships;
3. `reading_order`, `tests`, and `impact` — bounded paths for the next inspection decision; and
4. every `omitted` counter plus `validations` — explicit truncation and evidence-backed follow-up
   categories that RepoScout did not execute.

The report has a hard aggregate budget of 100 serialized path entries, at most 25 detailed graph
gaps, and at most 10 validation entries. `reposcout capabilities -f json` advertises these limits.
Matching tests are naming/convention evidence, not measured coverage. Validation entries never
claim that a command ran.

Use the detailed workflow when declaration outlines, complete context data, or the ordinary
change-scoped aggregate is needed:

```sh
reposcout -f json --summary --profile agent \
  --working --context --impact .
```

Each context evidence record exposes:

- its role (`changed`, `matching-test`, `dependency`, `dependent`, `nearby`);
- graph distance;
- resolver provenance when applicable; and
- `high` or `partial` confidence.

Deleted files remain valid topology seeds. A subpath target limits which changed files seed the
query, while matching dependents may live elsewhere in the repository.

## Graph coverage

`--graph` combines import topology and explicit type relationships across every first-class
language:

| Language | Local resolution |
|---|---|
| JavaScript / TypeScript | Relative imports, `tsconfig.json`/`jsconfig.json` paths and `baseUrl`, project references, local package exports/imports/entrypoints, and checked-in TypeScript behind runtime extensions |
| Python | Relative imports plus unambiguous repository-absolute and conventional `src/`-root modules |
| PHP | Composer PSR-4/PSR-0 autoload maps, conventional source roots, and static include/require paths |
| Rust | External modules, `#[path]`, local `crate`/`self`/`super` uses, and unambiguous Cargo-local library names |
| Go | `go.mod` module imports and relative packages, targeting a deterministic package representative |

Full machine output exposes path-sorted adjacency and edge records with resolver provenance.
Explicit `extends`, `implements`, trait, and embedding relations appear in separate symbol edges;
they are never folded into import fan-in.

Ambiguous short names, duplicate package names, invalid resolver configuration, syntax errors, and
unresolved local imports remain diagnostic. The graph does not invent confidence:

- Rust use edges are module-level rather than symbol-reference indexes.
- Go package imports point to a representative file, not every precise file reference.
- Generic recognized languages receive no fabricated graph coverage.

Focus a neighborhood and export it without external tooling:

```sh
reposcout --graph-focus src/service.ts --graph-direction dependents \
  --graph-depth 2 -f mermaid .
```

## Review changed findings

Fast review filters current findings to changed-line ranges:

```sh
reposcout --working --review .
```

Deep review compares Git snapshots with the same discovery and analysis policy:

```sh
reposcout -f sarif --since main --review=deep --fail-on-review .
```

Deep states are `new`, `resolved`, `worsened`, and `improved`. Rename identities are remapped
before comparison. `--fail-on-review` gates fast current findings or deep `new`/`worsened`
findings; resolved and improved findings remain informational.

## Explain one file

```sh
reposcout explain src/service.ts
reposcout explain src/service.ts -f json
```

`explain` scans the surrounding repository and projects onto one requested file:

- whether it was discovered and any exact ignore rule;
- metrics, complexity, churn, and risk factors;
- conventional test matches;
- direct dependencies and dependents; and
- related canonical findings.

## Choose the trust and cost profile

| Profile | Behavior |
|---|---|
| `full` | Runs the complete configured analysis |
| `agent` | Skips whole-corpus duplication and churn unless explicitly selected |
| `safe` | Also ignores repository configuration and applies conservative discovery, worker, history, context, and duplication limits |

Use `safe` when scouting an untrusted checkout:

```sh
reposcout -f json --summary --profile safe .
```

Explicit analyzer selection may opt back into an analyzer under the safe limits. The report's
`execution` and `analysis_profile` blocks disclose the effective configuration and unavailable
signals, so disabled work never appears as a confident zero.

## Read diagnostics before trusting absence

Every scan reports:

- discovered, analyzed, unsupported, and unreadable file counts, plus bounded path examples for
  unsupported files;
- oversized and resource-omitted file/byte counts;
- whether discovery or analysis stopped at a file, byte, or cooperative time limit;
- walker errors;
- cache hits, misses, and lazy enrichments;
- broad stage timings; and
- whether Type-2 candidate, match, or suppression bounds made duplication partial.

If `type2_analysis_partial` is true, exact-clone analysis is complete but Type-2 findings and the
combined duplication percentage are lower bounds.

## Suggested agent sequence

```text
capabilities
    ↓
compact summary scout
    ↓
locate / explain / focused context
    ↓
bounded change summary, detailed impact, or review when a change exists
    ↓
open only the selected source and tests
```

The sequence is guidance, not a protocol dependency. JSON/NDJSON and structured errors remain the
stable integration boundary.
