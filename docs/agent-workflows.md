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
blocks remain when requested, but compact context omits detailed declaration objects.

If the task needs only a few decisions, select those fields before the report enters agent
context. `jq -c` both filters and preserves compact JSON; bare `jq` only reformats the complete
payload:

```sh
reposcout -f json --summary --profile agent <PATH> \
  | jq -c '{diagnostics, assessment: .summary.assessment, source: .summary.source, work_scope}'
```

The most useful first-pass fields are:

| Question | Evidence |
|---|---|
| Will readable source fit? | `summary.source.tokens`, `summary.assessment` |
| How large is the complete inventory? | `summary.tokens`, `summary.files` |
| Is cleanup worthwhile? | `summary.assessment.production_duplication`, complexity violations, risks, and assessment reasons |
| What should be read first? | `summary.top_risks`, `summary.top_source_token_files` |
| What can be skipped? | `summary.skip_candidates` and each reason |
| Is a supported test runner configured, and what files do its defaults select? | `summary.test_presence` |
| Did the scan cover the target? | top-level `diagnostics` |
| Was Type-2 analysis complete? | `diagnostics.type2_analysis_partial` |

## Read the work scope before choosing depth

Every new scan report except `--baseline-ready` includes a versioned top-level `work_scope` block.
It projects facts already produced by the selected workflow; it never enables graph construction,
context planning, or another analyzer on its own.

Read it in this order:

1. `basis` and `inventory` identify whether the primary facts describe a repository or diff.
   `inventory.discovery_files` is the post-ignore repository discovery universe before an optional
   diff narrows the scan; `inventory.primary_files` is the repository or diff scope actually
   analyzed, while `source_files` and `source_tokens` describe that primary scope.
2. `production_duplication`, when present, gives production-source duplicated/analyzed line counts
   and an explicit completeness bit. A partial percentage is observed evidence, not a clean result.
3. `seeds` preserves exact focus/change totals, resolved versus unmatched focus inputs, bounded
   path examples, and omissions.
4. `context` reports selected, outline-only, omitted, and skipped files plus uncapped selected and
   omitted token totals.
5. `impact` reports graph-eligible/covered seed files, direct/transitive dependents, and whether
   matching-test counts were actually evaluated.
6. `structure` reports weakly connected components only when the selected workflow already built
   a graph. Components describe observed topology; they do not prove that work is independent.
7. `confidence` separates primary-scan and full planning-universe coverage, graph gaps, partial
   Type-2 analysis, and unavailable signals. `primary.diff_scoped` makes an intentional diff
   boundary explicit rather than presenting it as a discovery gap.

Path examples share one hard 25-entry budget and graph structure keeps at most 10 component
records; exact totals and omission counts survive either bound. Discover the installed version and
limits through `reposcout capabilities -f json`.

These are raw measurements. The calling agent decides whether the task fits its available context,
needs deeper inspection, or would benefit from splitting or delegation.

`summary.test_presence` exists only when discovered manifests or runner configuration establish a
supported test setup. It reports the evidence-backed runners and files selected by their default
discovery conventions; it does not infer source-to-test matches or measured coverage.

## Interpret quality evidence progressively

Start with the smallest decision-ready projections:

1. Use `summary.assessment.production_duplication` for cleanup decisions. It excludes conventional
   test files and direct Rust inline-test regions, preserves duplicated/analyzed line counts, and
   says whether the result is complete.
2. Use `summary.top_production_duplicates` for the first production blocks to inspect. Its
   redundancy filter suppresses nested/substantially overlapping rankings, and a family needs at
   least one instance with `min_dup_lines` contiguous non-test lines; neither rule changes raw
   detector output.
3. Use `summary.top_duplicates` when test/content relationships in the complete configured health
   corpus matter. Request full JSON or `--dup-details` only when every raw group, pair, or precise
   location is needed.
4. Use `summary.top_risks` as a ranking, not a severity verdict. Algorithm `5` uses continuous
   half-saturation factors, and each entry identifies `algorithm_version` plus its raw SLOC,
   cyclomatic, and churn inputs.

If production duplication has `complete: false`, report it as observed partial evidence. It is an
“at least” percentage only when diagnostics show Type-2 work was the sole gap; omitted source files
can change both numerator and denominator. If `top_production_duplicates` is absent but production
evidence is present, no production family survived the compact projection; do not reinterpret that
omission as a disabled analyzer.

## Discover capabilities

Do not hard-code assumptions about an installed binary:

```sh
reposcout capabilities -f json
```

This command performs no repository scan. It advertises commands, output/error formats, profiles,
language coverage, health scopes and path-exclusion support, symbol kinds, and hard graph,
duplication, change-summary, and work-scope bounds.

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

RepoScout computes bounded, body-free declaration outlines without embedding source bodies.
Summary output keeps `outline_symbols`, `outline_bytes`, and omission counts but drops each
file's `symbols` array. `outline_details_omitted: true` appears only when at least one declaration
object was actually removed. Omit `--summary` only when the actual declaration signatures are
needed. Neither form exceeds the requested aggregate source-token or file budget.

Project a focused reading plan when only selection evidence is needed:

```sh
reposcout -f json --summary --profile agent \
  --focus src/service.ts --context-budget 24000 --context-max-files 15 . \
  | jq -c '{diagnostics, work_scope, context: (.context | {
      budget_tokens, selected_tokens, omitted_tokens, files, omitted
    })}'
```

Focus one or more paths:

```sh
reposcout -f json --summary \
  --focus src/service.ts --focus tests/service.test.ts .
```

An oversized explicit focus may appear as `outline_only`: its source does not fit the budget, but
its path and selection evidence remain visible. Full JSON also carries its bounded declarations.
Unmatched focus values stay explicit rather than becoming invented seeds.

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
reposcout -f json --profile agent \
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
combined duplication percentage are lower bounds. Read
`summary.assessment.production_duplication.complete` for the corresponding production-source
qualification; other discovery/read limits can also make it partial.

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
