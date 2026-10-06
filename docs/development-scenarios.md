# Development scenarios

These opt-in tests exercise RepoScout's public CLI against purposeful synthetic repositories.
Focused scenarios and command chains connect operations that can each work alone while disagreeing
at their boundaries: a review report and its snapshot read, an import graph and its change context,
or a warm cache and a changed health policy.

They check the public CLI contract. Expected paths, declarations, references, statuses and metric
counts come from authored fixtures and documented rules, not copies of current CLI output. A
focused scenario can require exact results, with distractors and negative controls exposing
incorrect extra evidence. Metamorphic checks compare forwards/backwards changes, filename
permutations, dirty worktrees, and cached/uncached analysis where the contract requires equivalent
results.

The separate [user acceptance cases](user-acceptance-cases.md) freeze five user needs and their
domain truth before CLI adaptation. They require enough truthful evidence for a stated decision;
passing command contracts alone does not establish that outcome. Modest relevant extra context is
allowed, subject to the shipping case's explicit reading budget.
See [acceptance results](user-acceptance-results.md) for observed passes, remaining RED criteria
and reproduction commands.

The next [user efficiency cases](user-efficiency-cases.md) are a design-only extension: six further
information needs with budgets for complete interactions, minimal source and evidence reuse.
They have not been implemented or run and do not change the existing acceptance baseline.

## Run

```sh
./scripts/test-scenarios.sh --list
./scripts/test-scenarios.sh
./scripts/test-scenarios.sh revisions
./scripts/test-scenarios.sh navigation
./scripts/test-scenarios.sh inventory
./scripts/test-scenarios.sh boundaries
./scripts/test-scenarios.sh journeys
./scripts/test-scenarios.sh acceptance
./scripts/test-scenarios.sh --keep-failed SCENARIO_NAME
```

The script first refreshes the release CLI, compiles the test target, rejects a filter that matches
no scenarios, and runs the selected tests. Compilation time is separate from test execution time.
Rust/Cargo and a Unix host are required; the test runtime does not need network services, external
projects, package installations, a daemon or a frontend server.
The `acceptance` group also needs `python3` for bounded standard-library fixture probes. A missing
interpreter is an environment failure and must not be reported as a product acceptance failure.

`cargo test` still compiles these tests to catch drift, but `#[ignore]` keeps every scenario out of
the default test run and existing CI. To run the target directly:

```sh
cargo test --release --locked --test development_scenarios -- --ignored --nocapture
```

No fast subset is automatically promoted to CI. That is a separate decision after measuring its
runtime and reliability on the supported runners.

Complete Linux validation runs on 2026-10-05 passed the original 20 scenarios in 83–106 seconds after
compilation. These are development-machine observations, not a performance requirement or a CI forecast.
The normal target invocation separately confirmed that all 20 remain ignored without opt-in.
The focused guard-mapping, budget-fairness and snapshot-handoff scenarios also ran against a previously
verified official v0.4.0 binary: each failed on its corresponding known behavior defect, while
the current binary passed. No old binary or network download is required by the suite itself.

## Scenario families

The [command-chain journeys](development-journeys.md) chain actual CLI responses through
review, navigation, diagnostic and cleanup workflows. Their drivers receive task inputs only;
independent oracles check the reached evidence without supplying hidden follow-up targets.
The seven chains retain interface coverage; their historical validation results are recorded in
the linked guide and do not establish outcomes for the new acceptance cases.

| Family | Behaviors under test |
| --- | --- |
| Revisions | Body edits and declaration counterparts, revision-local resolution, rename/add/delete, direct versus merge-base comparisons, output-budget fairness, and historical reads after live filesystem changes |
| Navigation | Search/read identity, call consumers and definition plans, aliases/shadowing, import impact and context, nested Godot projects, and whole-definition source admission |
| Inventory | Source/content health separation, comment-aware markers, artifact duplication policy, cache equivalence/invalidation, exact output exclusion, and baseline gates |
| Boundaries | Hierarchical ignore policy across scans and snapshot reads, project-configuration trust, and explicit input-limit gaps without losing small files |
| Acceptance | Refund-boundary and helper-deletion review evidence, shipping investigation, tariff centralization, and new tariff debt despite improved aggregate totals |

Large here means connected behavior and enough surrounding code to expose misleading shortcuts,
not uncontrolled repository size. Repositories are generated from readable Rust fixture builders;
there are no opaque downloaded corpora or automatically accepted report snapshots.

## Isolation and diagnosis

Each fixture owns a temporary repository plus separate temporary cache/configuration storage. The
shared CLI command helper retains the two-worker test configuration. Cargo's repository setting
serializes tests; each CLI command has a 30-second timeout. Do not raise test parallelism when
running this suite. Resource limits in [agent validation](agents/validation.md) still apply.

Captured stdout is decoded as JSON; assertions report the concrete violated expectation. With
`--nocapture`, the fixture label, commands and fixture elapsed times are printed alongside Rust's
test names and final duration. These are diagnostic timings, not stable performance thresholds.

Fixtures normally clean themselves up, including on assertion failure. `--keep-failed` retains
only fixtures unwinding from a failed test and prints their location. Their `repository/` and
`state/` directories contain synthetic input and private cache state. A timed-out/killed test
process may leave temporary directories; delete only the exact paths belonging to that run.

Historical contents are pinned; current ignore policy still applies. The snapshot journey checks
missing and symlink-shaped live source parents under normal policy. A non-directory parent also
prevents reading nested current ignore files; the extra physical-source check uses the explicit
safe profile, which disables repository-owned ignore loading. This separates source identity from
policy availability instead of treating an unavailable policy as an empty set of rules.

## Extend the suite

Put a scenario in the appropriate file under `tests/development_scenarios/` and give it an explicit
ignore reason. Use `support::Fixture` and the existing CLI helper; do not add a separate scanner,
resolver, cache or expected-output generator. Keep each journey's independent expected results
close to its fixture, and assert availability/omissions as well as successful data.

For focused contract scenarios, prefer exact evidence sets and hand-derived discrete counts.
Exclude timestamps and resource
timings from semantic equivalence checks. Assert an empty set only when the relevant extraction,
resolution and output coverage is complete. If a documented behavior fails, preserve the failure
as evidence and investigate it rather than weakening the assertion to match today's output.

For acceptance cases, keep the frozen user inputs and evidence obligations separate from the CLI
driver. Independent application probes check the authored truth, not expected RepoScout output.
Missing necessary evidence is an explicit RED even when the CLI reports its limits honestly; keep
the unmet criterion and reproducer rather than using an expected-failure wrapper. The acceptance
group remains opt-in and adds no automatic CI execution.

These tests supplement focused regressions. They do not establish runtime impact completeness,
real test coverage, security certification, or whole-project correctness.
