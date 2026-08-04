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

The default duplication corpus excludes minified and recognized bundled/chunk output. Those files
remain part of repository inventory and navigation, but do not consume duplication-tokenization
work or contribute to coverage denominators and findings. `--dup-include-artifacts` is the explicit
opt-in for specialized scans that need them.

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

`summary.top_duplicates` ranks clone families by removable lines across the configured health
corpus. It is a compact projection: after retaining the highest-impact family, a later family must
add at least `min_dup_lines` contiguous uncovered lines in at least two instances. This suppresses
nested or substantially overlapping blocks without changing the detector result.

`summary.top_production_duplicates` applies the same compact ranking to families that touch
production source. Families found only in conventional test files (including Rust split modules
named `tests.rs`) or direct Rust `#[cfg(test)]` regions are omitted. An instance intersecting an
inline-test region must retain at least `min_dup_lines` contiguous non-test lines to count as
production; a mixed production/test family remains visible. Table and Markdown reports use this
production projection by default.

Full JSON retains every exact/near group in `duplicates`, stable pair findings, precise locations,
and union coverage. `--dup-details` expands human reports from the same raw pair findings. Compact
projection filtering never removes or rewrites that evidence.

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

Source-file risk algorithm `5` is a stable, continuous composite:

```text
0.40 × size + 0.40 × complexity + 0.20 × churn

factor(value, anchor) = value / (value + anchor)
```

The half-saturation anchors are 1,000 SLOC, cyclomatic `100`, and `20` commits: each factor is
`0.5` at its anchor and continues increasing smoothly above it rather than becoming tied at a hard
cap. Compact risk entries and detailed file explanations carry `algorithm_version` plus the raw
SLOC, cyclomatic, and commit inputs; deterministic ties fall back to those inputs and then path.
The canonical risk-finding threshold remains `0.7`, and a risk-algorithm change makes finding
profiles baseline-incompatible.

Risk reasons expose the contributing raw signals. A missing conventional test filename remains
visible as navigation evidence, but it does not change the risk score because filename matching is
not measured coverage.

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

Its `production_duplication` evidence records the `production-source` corpus, duplicated/analyzed
line counts, percentage, and whether that value is complete. It excludes separate test files and
direct Rust `#[cfg(test)]` regions. Raw duplication metrics and findings still cover the complete
set of analyzed files in the configured health corpus.

Production duplication is marked partial when Type-2 analysis stopped at a safety bound or
recognized source evidence was lost to unreadable files, walker errors, file/byte limits, or the
scan-duration limit. The percentage is then observed partial evidence rather than a complete clean
result. When only Type-2 work is partial and the source corpus is complete, it is a lower bound;
when source files were omitted, the complete-repository percentage may move in either direction.
A churn-only truncation does not make duplication evidence partial because it does not change the
duplication corpus.
Complete repository token totals remain available as `summary.tokens`; context fit deliberately
uses `summary.source.tokens` so large data, prose, and other content assets do not create a false
source-reading overflow.

Evidence qualifiers prevent disabled analysis from becoming a synthetic clean result:

- `fits_context_known` is false without token analysis;
- `cleanup_worth_complete` is false when required health evidence was disabled or production
  duplication is partial; and
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
