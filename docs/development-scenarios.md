# Development scenarios

These opt-in tests exercise complete RepoScout user journeys against purposeful synthetic
repositories. They connect operations that can each work alone while disagreeing at their
boundaries: a review report and its snapshot read, an import graph and its change context, or a
warm cache and a changed health policy.

They check the public CLI contract. Expected paths, declarations, references, statuses and metric
counts come from authored fixtures and documented rules, not copies of current CLI output. A
scenario includes relevant distractors and negative controls so that returning extra evidence is
also a failure. Metamorphic checks compare forwards/backwards changes, filename permutations,
dirty worktrees, and cached/uncached analysis where the contract requires equivalent results.

## Run

```sh
./scripts/test-scenarios.sh --list
./scripts/test-scenarios.sh
./scripts/test-scenarios.sh revisions
./scripts/test-scenarios.sh navigation
./scripts/test-scenarios.sh inventory
./scripts/test-scenarios.sh boundaries
./scripts/test-scenarios.sh journeys
./scripts/test-scenarios.sh --keep-failed SCENARIO_NAME
```

The script first refreshes the release CLI, compiles the test target, rejects a filter that matches
no scenarios, and runs the selected tests. Compilation time is separate from test execution time.
Rust/Cargo and a Unix host are required; the test runtime does not need network services, external
projects, package installations, a daemon or a frontend server.

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

The additional [end-to-end journeys](development-journeys.md) chain actual CLI responses through
complete review, navigation, diagnostic and cleanup tasks. Their drivers receive task inputs only;
independent oracles check the reached evidence without supplying hidden follow-up targets.
The seven journeys bring this opt-in target to 27 scenarios; their separate validation results
are recorded in the linked journey guide.

| Family | Behaviors under test |
| --- | --- |
| Revisions | Body edits and declaration counterparts, revision-local resolution, rename/add/delete, direct versus merge-base comparisons, output-budget fairness, and historical reads after live filesystem changes |
| Navigation | Search/read identity, call consumers and definition plans, aliases/shadowing, import impact and context, nested Godot projects, and whole-definition source admission |
| Inventory | Source/content health separation, comment-aware markers, artifact duplication policy, cache equivalence/invalidation, exact output exclusion, and baseline gates |
| Boundaries | Hierarchical ignore policy across scans and snapshot reads, project-configuration trust, and explicit input-limit gaps without losing small files |

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

Prefer exact evidence sets and hand-derived discrete counts. Exclude timestamps and resource
timings from semantic equivalence checks. Assert an empty set only when the relevant extraction,
resolution and output coverage is complete. If a documented behavior fails, preserve the failure
as evidence and investigate it rather than weakening the assertion to match today's output.

These tests supplement focused regressions. They do not establish runtime impact completeness,
real test coverage, security certification, or whole-project correctness.
