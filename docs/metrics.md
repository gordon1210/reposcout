# Metrics and interpretation

← [Documentation index](README.md)

RepoScout's metrics are consistent repository-scouting signals. They are not substitutes for
language-specific analyzers such as ESLint, Sonar, or Visual Studio.

## Inventory versus health

Every recognized format contributes:

- file and byte totals;
- token/context size; and
- LOC, SLOC, blank, and comment facts.

Health analysis is source-first. Complexity, markers, duplication, risk, test-presence, and cleanup
signals default to programming languages, SQL, Dockerfiles (including `Dockerfile.*` variants),
and Makefiles. HTML, CSS/SCSS, JSON, YAML, TOML, Markdown, XML, and text require
`--health-include <FORMAT>` or `--health-scope all`.

This separation prevents documentation and generated data from diluting code-health percentages
while retaining complete repository inventory.

`--health-exclude <GLOB>` removes selected repository-relative paths from health analysis without
removing their inventory, token, line, navigation, import, symbol, or context facts. Selection
order is scope, then format includes, then path excludes; an exclude always wins. Ordinary
`--exclude` is different because it removes the path from the whole scan.

## Line metrics

Rust, Python, JavaScript, TypeScript/TSX, Go, and PHP classify comments from tree-sitter ranges.
Comment delimiters inside strings therefore remain code.

Other formats use a quote-aware fallback. Full JSON marks those files with
`line_metrics_approximate: true`, and the summary reports
`line_metrics_approximate_files`.

## Complexity

Complexity is calculated for code only. Markdown, JSON, YAML, HTML, CSS, and other prose/data
formats receive no complexity object and never enter hotspot rankings.

For first-class languages, RepoScout records each named or anonymous callable independently:

- functions and methods;
- Rust closures;
- Python lambdas;
- JavaScript/TypeScript arrows and function expressions;
- Go function literals; and
- PHP closures and arrows.

### Cyclomatic complexity

`--max-complexity` is an ESLint-style reporting threshold. The default maximum is `20`, so a
callable at exactly `20` is allowed and a callable at `21` is reported.

File-level cyclomatic values aggregate independent callable scopes plus top-level decisions.
Summary averages and gates remain callable-based:

```sh
reposcout complexity --max-complexity 12 src/
reposcout --max-complexity 12 --fail-on "max-cyclomatic>12" src/
```

Python comprehension clauses and modern JavaScript/TypeScript defaults, logical assignments,
optional chains, and nullish coalescing contribute control-flow paths.

### Cognitive complexity and nesting

Cognitive complexity emphasizes nested control flow and includes direct self-recursion. It does
not attempt to model mutual-recursion cycles or every language-specific Sonar nuance.

### Maintainability Index

RepoScout uses Microsoft's normalized `0..100` formula, with SLOC as a cross-language
source-operation proxy:

| Score | Interpretation |
|---|---|
| `0..9` | Low |
| `10..19` | Moderate |
| `20..100` | Good |

Comments do not directly increase the score.

### Halstead signals

Volume, difficulty, and effort follow the published Halstead equations. Operator/operand
classification comes from grammar leaf tokens or the generic fallback, so compare these signals
within RepoScout rather than directly across tools or languages.

## Duplication

Duplication matches structured token sequences, not semantics.

| Detector | Behavior |
|---|---|
| Exact / Type-1 | Preserves identifier and literal values |
| Near / Type-2 | Allows consistent identifier renames and same-category literal changes |

The structured lexer retains precise line, column, byte, and token ranges. Candidate pools are
isolated by format unless `compatible` or `all` scope is requested.

Type-2 similarity is weighted:

- exact tokens: `1.0`;
- consistently renamed identifiers: `0.80`; and
- changed literals of the same category: `0.70`.

`summary.top_duplicates` ranks clone families by removable lines. Full reports also expose stable
pair findings and precise locations.

### Coverage semantics

Line and duplication-lexer-token percentages use physical unions over the eligible corpus:

- overlapping exact and near findings are not double-counted;
- overlapping instances in one file do not become fake extra copies; and
- excluded content cannot dilute the denominator.

Repeated-but-not-extractable text, such as identical import preambles, can still be a valid
high-ranking match. Exclude vendored/generated trees with `reposcout.toml`,
`.reposcoutignore`, or `--exclude`.

### Type-2 safety bounds

Each format pool admits work rarest-first and stops at:

| Bound | Maximum |
|---|---:|
| Candidate seed pairs | 10,000,000 |
| Buffered compact matches | 250,000 |
| Suppression overlap checks | 10,000,000 |

Ordinary repositories below those limits retain complete results. If a bound is reached:

- exact analysis remains complete;
- Type-2 findings and combined percentages become lower bounds;
- table/Markdown reports say the result is partial; and
- JSON records the reason plus omitted pair/match counts.

The installed limits are available through `reposcout capabilities -f json`. There is no CLI
override in v0.1.

## Markers

TODO/FIXME/HACK-style markers are comment-aware in first-class languages. Identifiers, strings,
template strings, Python docstrings, and TSX text do not create findings.

Opted-in generic/content formats and parse failures use a raw-text fallback. Every precise marker
occurrence contributes to the canonical finding catalog.

## Churn and hotspots

Git churn records commit count, authors, and first/last change dates for discovered paths.
Hotspots combine churn with complexity for code files only. A merge commit does not count as a
second touch when the authored branch commit already represents the same path change.

History is bounded by `churn_max_commits` unless explicitly configured otherwise. The immutable
per-commit history index is cached separately from file analysis.

## Risk

Source-file risk is a stable composite:

```text
0.40 × size + 0.40 × complexity + 0.20 × churn
```

Inputs saturate at 1,000 SLOC, cyclomatic `100`, and `20` commits. Risk reasons expose those
contributing factors. A missing conventional test filename remains visible as navigation evidence,
but it does not change the risk score because filename matching is not measured coverage.

## Test presence

RepoScout classifies test files and estimates source/test matches from conventional names and
logical directories. Package prefixes remain part of the match key, so identically named files in
different monorepo packages do not cross-match.

Rust inline `#[test]` and `#[cfg(test)]` blocks can satisfy the heuristic. PHPUnit
`SomethingTest.php` and common `.test`, `.spec`, `test_`, and `_test` conventions are recognized.
For Rust binary crates, `tests/cli.rs` conventionally matches the package `src/main.rs`
entrypoint.

Serialized `untested_*` names are retained for schema compatibility and mean only “no matching test
was found.” This heuristic does not change risk or cleanup scoring.

## Assessment

`summary.assessment` is computed after other signals and answers:

- whether readable source/build tokens appear to fit a context budget;
- whether cleanup value looks low, medium, or high; and
- why.

Its source-duplication signal excludes separate test files and direct Rust `#[cfg(test)]` regions.
Raw duplication metrics and findings still cover the complete configured health corpus.
Complete repository token totals remain available as `summary.tokens`; context fit deliberately
uses `summary.source.tokens` so large data, prose, and other content assets do not create a false
source-reading overflow.

Evidence qualifiers prevent disabled analysis from becoming a synthetic clean result:

- `fits_context_known` is false without token analysis;
- `cleanup_worth_complete` is false when required health evidence was disabled; and
- `unavailable_signals` lists missing inputs.

The assessment's duplication reason uses non-test source only and triggers above 15%. Filename
test matching is informational and does not become a cleanup reason. Raw duplication summaries
still cover the effective health corpus.

## Known interpretation limits

- Generic line and complexity fallbacks are explicitly approximate.
- Maintainability Index uses cross-language SLOC rather than a language-specific logical-operation
  count.
- JavaScript class-field initializers and static blocks are not modeled as separate implicit
  callable scopes.
- Composite risk, test matching, Halstead classification, and assessment are RepoScout-specific
  heuristics.
- Dependency/type graphs are conservative and diagnostic-first; unresolved or ambiguous evidence
  remains visible instead of becoming a guessed edge.

Use these metrics to rank and compare work inside RepoScout, then use specialist tools for
language-specific enforcement when needed.
