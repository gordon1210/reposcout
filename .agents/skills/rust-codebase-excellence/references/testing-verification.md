# Testing and Verification

Read this reference for test design, regression coverage, linting, feature and target matrices, CI, documentation checks, code coverage, fuzzing, Miri, and concurrency verification.

## 1. Verification starts from risk

Choose checks from the contracts touched by the change:

| Change | Minimum focused verification |
|---|---|
| Pure internal logic | Unit or integration regression test plus normal lint/check suite |
| Public library API | Integration and doc tests, public documentation build, SemVer review |
| Cargo features or optional dependencies | Supported feature matrix and target-specific checks |
| MSRV-sensitive code or dependencies | Actual MSRV build/check/test as promised |
| Async or concurrent behavior | Cancellation, shutdown, saturation, and race-oriented tests |
| Parser, decoder, deserializer, protocol | Malformed, truncated, oversized, adversarial, and round-trip tests; fuzz where valuable |
| `unsafe` or FFI | Safety-focused tests, Miri where supported, target ABI checks, fuzz/model tests as applicable |
| Performance fix | Reproducible benchmark plus correctness suite |
| Storage or wire format | Compatibility fixtures, migrations, old/new version tests |
| CLI or service behavior | Exit/status mapping, stdout/stderr or protocol response, shutdown and config tests |

Do not substitute a large generic test run for a focused regression test that proves the intended behavior.

## 2. Test layers

### Unit tests

Use for:

- Pure functions and local invariants.
- Error classification and boundary cases.
- Private implementation details whose behavior is stable enough to matter.
- Fast combinatorial coverage.

Keep unit tests near the code when they benefit from private access. Do not expose internals publicly merely to test them.

### Integration tests

Use for:

- Public crate API.
- Multiple modules or packages interacting.
- Filesystem, database, process, network, configuration, and protocol boundaries.
- Binary behavior using Cargo-provided binary paths where applicable.

Integration tests should consume the crate as an external caller would. They are the best guard against accidental public-surface breakage.

### Documentation tests

Use for public examples that should compile and remain current. A doc test is valuable when it teaches normal use, not when it exists solely to raise coverage.

Mark examples deliberately:

- Runnable by default when practical.
- `no_run` when compilation matters but execution needs external state.
- `compile_fail` for stable misuse guarantees.
- `ignore` only when the environment genuinely prevents useful verification; explain why.

### End-to-end tests

Use sparingly for critical user journeys and deployment boundaries. Keep lower-level logic covered by faster tests so end-to-end failures remain diagnosable.

## 3. Test behavior, not implementation trivia

A good test states one coherent behavior and may contain multiple related assertions.

Prefer:

- Input, operation, observable result.
- Domain names and failure reasons.
- Stable semantic assertions.
- Explicit regression setup for the bug being fixed.

Avoid:

- One test per line or getter.
- Tests coupled to private call ordering without a contract.
- Exact full error strings when structured classification is available.
- Broad snapshots where a few semantic assertions would be clearer.
- Tests that pass even if the intended behavior is removed.

When fixing a bug, prove the test fails against the old behavior when practical.

## 4. Determinism and isolation

Tests may run concurrently and in arbitrary order.

- Do not share fixed filenames, ports, database rows, environment variables, current directories, or process-global state without serialization and cleanup.
- Use unique temporary directories and let RAII clean them up.
- Bind ephemeral ports rather than guessing a free port, while accounting for handoff races.
- Use seeded randomness and print or preserve the seed on failure.
- Inject clocks or use runtime paused time instead of sleeping.
- Avoid ambient network access in unit tests.
- Bound all waits; a test should fail rather than hang forever.
- Restore modified environment or process state even on panic where possible.
- Keep fixtures minimal and explicit. Large opaque fixtures hide why a test failed.

If code inherently mutates process-global state, isolate it in a separate test process or serialize only that test group.

## 5. Error and boundary coverage

For each changed operation, consider:

- Empty and minimum input.
- Maximum accepted input and one beyond it.
- Malformed, truncated, duplicated, reordered, and unknown input.
- Unicode, non-UTF-8 OS strings, path separators, and platform-specific values where relevant.
- Integer boundaries, overflow, negative values, and conversion failure.
- Missing, stale, conflicting, or partially written state.
- Permission, timeout, cancellation, closure, and downstream failure.
- Retry after known-not-applied and unknown-outcome failures.
- Cleanup after every failure point.

Test only relevant categories, but make the omission deliberate.

## 6. Property, round-trip, and fuzz testing

Property tests are useful when many inputs share a compact invariant:

- Parse/serialize round trips.
- Normalization idempotence.
- Ordering or set laws.
- State-machine transitions.
- Arithmetic invariants.
- Equivalence between a reference implementation and optimized implementation.

Fuzzing is valuable for:

- Parsers, decoders, protocol frames, binary formats, and unsafe boundaries.
- Inputs with high combinatorial structure.
- Code where a panic, hang, excessive allocation, or undefined behavior is a security concern.

A fuzz target should:

- Be deterministic for one input.
- Avoid unbounded external side effects.
- Assert useful invariants, not only “does not crash.”
- Minimize or retain regression inputs discovered by the fuzzer.

Do not add fuzz infrastructure to trivial code without a risk-based reason.

## 7. Snapshot tests

Use snapshots for complex, human-reviewed output such as:

- Compiler diagnostics.
- Generated documents.
- Structured command output.
- Large protocol renderings with a stable representation.

Rules:

- Review snapshot diffs as code.
- Normalize timestamps, paths, random IDs, and unstable ordering.
- Keep explicit assertions for security, status, or key semantic fields.
- Avoid giant snapshots that make meaningful changes invisible.
- Do not auto-accept snapshots in CI.

## 8. Compile-time tests

Use compile-pass or compile-fail tests when the contract is type-level:

- Macros and diagnostics.
- Trait implementation availability.
- Typestate or ownership misuse.
- Feature-gated API presence.
- Public API examples.

Keep expected diagnostics resilient to compiler wording changes unless exact diagnostics are the product, as with a proc macro.

## 9. Baseline command sequence

Use repository scripts and CI definitions first. Adapt commands to package, target, feature, and lockfile policy.

A common stable baseline is:

```bash
cargo fmt --all -- --check
cargo check --workspace --all-targets
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo test --workspace --doc
cargo doc --workspace --no-deps
```

This is a template, not a universal command block:

- Add `--locked` when the repository intentionally commits and enforces `Cargo.lock`.
- Do not use `--workspace` when only selected packages are supported on the current host.
- Do not use `--all-targets` when examples or benches intentionally need unavailable services, features, or target toolchains; test those in their proper jobs instead.
- Do not add `--all-features` unless all features are designed to coexist.
- Apply package and feature selectors matching CI.
- Use `RUSTDOCFLAGS="-D warnings"` when the repository deliberately treats documentation warnings as errors.

Run focused tests first for fast feedback, then the broadest applicable suite before completion.

## 10. Clippy policy

- Start with `clippy::all`, which is intended to be broadly useful.
- Curate individual `pedantic`, `nursery`, or `restriction` lints. Never enable the restriction group wholesale.
- Treat warnings as errors in owned CI scope with `-D warnings`, not necessarily through crate-level `deny(warnings)`.
- Keep lint configuration in Cargo manifests when supported and useful; use `clippy.toml` only for configurable lint values.
- Set or inherit the project MSRV so Clippy does not recommend unavailable language features.
- Fix warnings unless the lint is demonstrably inapplicable.
- Place suppressions narrowly, include the reason, and prefer `#[expect]` only when the MSRV supports it and the lint should still fire at that location.
- Do not rewrite clear code merely to satisfy a low-value optional lint. Curate the policy instead.

Review newly introduced lint groups when upgrading toolchains; Clippy changes over time.

## 11. Feature verification

Build and test the supported matrix, not an imagined universal matrix.

Possible jobs:

```bash
cargo check --workspace
cargo check --workspace --no-default-features
cargo check -p package --features feature-a
cargo check -p package --features "feature-a feature-b"
cargo check --workspace --all-features  # only if valid by design
```

For libraries with several independent features, `cargo-hack` can automate each-feature or powerset checks when already adopted or explicitly approved. Exclude intentionally incompatible or target-specific combinations.

Verify examples, tests, and doc tests under the features they require. `required-features` can prevent invalid Cargo targets from being selected accidentally.

## 12. MSRV verification

- Use the exact promised MSRV toolchain.
- Check all published packages and their supported feature sets.
- Confirm dependencies resolve to MSRV-compatible versions.
- Do not pass `--ignore-rust-version` to make a promised MSRV job green.
- Keep MSRV failures separate from latest-stable failures so dependency resolution and language compatibility remain diagnosable.
- When a dependency update raises MSRV, treat it as a compatibility change even if application code did not change.

## 13. Target and platform verification

Compile or test on every supported platform where behavior depends on:

- Native libraries or FFI.
- Filesystem and path semantics.
- Process and signal APIs.
- Atomic availability.
- Endianness or pointer width.
- WASM or `no_std` support.
- Target-specific dependency graphs.

Cross-compilation verifies compilation, not runtime behavior or ABI integration. Run on real targets or representative emulation when runtime semantics matter.

## 14. Optional verification tools

Use only when installed, configured, or approved. Do not install tools silently.

- **cargo-nextest:** faster and more configurable test execution; retain any doc-test or special harness checks not covered by the configured runner.
- **cargo-llvm-cov:** source-based coverage diagnostics.
- **cargo-semver-checks:** automated public API compatibility checks for releases.
- **cargo-hack:** feature-set and powerset checking.
- **cargo-audit / cargo-deny:** advisories, licenses, sources, and dependency policy.
- **Miri:** dynamic undefined-behavior detection, especially for unsafe code; currently requires a supported nightly setup.
- **Loom:** controlled schedule exploration for concurrency primitives.
- **cargo-fuzz or other fuzz harnesses:** adversarial input exploration.
- **Criterion or an established benchmark harness:** statistically useful performance comparisons.

Tool output is evidence, not proof of complete correctness.

## 15. Coverage

Coverage helps locate unexercised code. It does not measure assertion strength, state-space coverage, race coverage, or requirement correctness.

- Do not optimize tests for a percentage alone.
- Investigate uncovered error, cleanup, and boundary paths first.
- Exclude generated or unreachable code only with a documented reason.
- Prefer branch or region insight over line-count vanity when available.
- Do not block a small project on an arbitrary threshold unless the team has a deliberate policy.

Mutation testing can reveal weak assertions when the cost is justified, but it is optional and should target important logic.

## 16. Flaky tests

A retry can diagnose flakiness but must not normalize it.

When a test flakes:

1. Preserve failure output and seed or schedule information.
2. Identify time, ordering, shared-state, external-service, or resource assumptions.
3. Make the dependency deterministic or isolate it.
4. Keep retries temporary or restricted to known external instability.
5. Track and remove the underlying cause.

A test that passes on retry is still a quality signal.

## 17. CI design

A proportionate small-to-medium project pipeline usually separates:

1. Formatting.
2. Fast check and lint.
3. Tests and doc tests.
4. Feature and target matrix.
5. MSRV, if promised.
6. Dependency/security policy.
7. Specialized unsafe, fuzz, concurrency, coverage, or benchmark jobs as risk requires.

Pin third-party CI actions and tool versions according to repository policy. Cache only safe build artifacts and keys; cache correctness must not affect correctness of the build.

Use fail-fast for fast independent jobs where useful, but preserve enough matrix output to diagnose platform-specific failures.

## Verification completion checklist

- A focused regression test proves the requested behavior.
- Tests are deterministic, isolated, bounded, and parallel-safe.
- Error, cleanup, and boundary paths relevant to the change are covered.
- Format, compile, lint, tests, docs, and feature checks match repository policy.
- MSRV and target contracts are tested when affected.
- Specialized tools are used only where risk justifies them.
- Every reported check was actually executed; gaps are explicit.
