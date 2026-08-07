# Tooling and Validation Baseline

Read this reference when selecting Cargo commands, defining CI, curating lints, or choosing optional Rust ecosystem tools. Repository-local commands and support policy take precedence.

Do not install tools, update toolchains, or execute Cargo in an untrusted repository without approval and appropriate isolation. Builds may execute build scripts and procedural macros.

## 1. Command selection principles

- Use the repository's scripts, task runner, and CI commands first.
- Run the narrowest useful check early, then expand to the supported matrix.
- Keep formatting, compile, lint, test, docs, feature, target, MSRV, security, and compatibility checks conceptually separate so failures are diagnosable.
- Do not use `--all-features` unless all features are designed to coexist.
- Do not assume default features cover supported minimal or alternative configurations.
- Do not treat optional third-party tools as mandatory unless the repository has adopted and pinned them.
- Record exact commands actually executed.

## 2. Portable baseline commands

Adapt package, target, feature, profile, and workspace flags to the repository.

### Formatting

```bash
cargo fmt --all -- --check
```

Use the repository's pinned toolchain and `rustfmt.toml`. Do not manually restyle around rustfmt.

### Compilation

```bash
cargo check --workspace --all-targets
```

Caveats:

- Add `--locked` when a committed lockfile is intentionally authoritative for the job. Cargo errors if the lockfile is absent or resolution would change; it is not a substitute for the project's library lockfile policy.
- `--all-targets` may build examples, benches, and tests that require optional environment or features; follow repository support claims.
- Cross-target validation may require `--target <triple>` and target-specific system dependencies.

### Clippy

```bash
cargo clippy --workspace --all-targets -- -D warnings
```

Curate exceptions and additional lint groups in workspace metadata or source attributes. Do not apply every pedantic or restriction lint as an error by default.

### Tests

```bash
cargo test --workspace
```

Add `--all-targets`, target filters, feature flags, or ignored suites only when that matches repository semantics.

### Documentation

```bash
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps
cargo test --doc --workspace
```

Published libraries may need feature-specific documentation and docs.rs metadata.

## 3. Feature matrix

Features are additive within a Cargo resolution. Design features to compose when practical, but test only combinations the repository supports.

At minimum consider:

```bash
cargo check --workspace --no-default-features
cargo check --workspace
cargo check --workspace --features '<supported-set>'
```

Use `--all-features` only when it is a supported combination:

```bash
cargo check --workspace --all-features
```

For mutually exclusive backends or modes:

- Prefer redesigning to additive capabilities when feasible.
- Otherwise reject invalid combinations with a clear `compile_error!` and test each supported combination separately.
- Document which package owns each feature and how workspace feature unification affects builds.

## 4. MSRV validation

- Read `package.rust-version`, workspace inheritance, `rust-toolchain.toml`, CI, and release policy.
- Compile with the declared minimum toolchain when MSRV is a contract.
- Ensure dependencies can resolve to versions compatible with that MSRV under the repository's resolver and lockfile policy.
- Avoid using syntax, standard-library APIs, Cargo keys, or lint attributes stabilized after MSRV.
- `#[expect]` requires Rust 1.81 or newer; use a narrowly justified `#[allow]` when supporting an older MSRV.

Typical CI shape:

```bash
cargo +<msrv> check --workspace --all-targets
cargo +stable test --workspace
```

Do not silently raise MSRV to satisfy a new dependency or language feature.

## 5. Target matrix

Derive targets from actual support claims, not every Rust target.

For each supported target, decide whether CI performs:

- `cargo check` only.
- Native tests.
- Cross-compiled tests under emulation.
- No-std or custom target builds.
- WASM component or browser/runtime tests.
- Platform integration tests.

Example:

```bash
cargo check --workspace --target <target-triple>
```

Review target-specific `cfg`, dependencies, path handling, filesystem behavior, sockets, atomics, endianness, pointer width, and C ABI assumptions.

## 6. Lint policy

A good lint policy is explicit, stable enough for CI, and owned by the workspace.

Recommended posture:

- Deny compiler and selected Clippy warnings in CI via `-D warnings`.
- Enable individual additional lints that prevent observed defects or enforce a deliberate policy.
- Keep broad lint groups at `warn` while curating them, or avoid enabling them globally.
- Use narrow, reasoned suppressions.
- Prefer `#[expect(lint, reason = "...")]` when MSRV supports it and the warning is intentionally expected.
- Avoid `#![deny(warnings)]` in published libraries because new compiler warnings can break downstream builds independently of the library release.
- Keep generated or foreign code outside normal lint scope where appropriate rather than filling it with suppressions.

Example workspace policy, only when compatible with MSRV and repository needs:

```toml
[workspace.lints.rust]
unsafe_op_in_unsafe_fn = "warn"
unexpected_cfgs = { level = "warn", check-cfg = [
  'cfg(loom)',
] }

[workspace.lints.clippy]
dbg_macro = "warn"
todo = "warn"
undocumented_unsafe_blocks = "warn"
```

Each member opts into inherited workspace lints:

```toml
[lints]
workspace = true
```

Do not paste this configuration blindly. Check lint availability against MSRV and whether generated/test code needs scoped exceptions.

## 7. Lockfile and reproducibility

- Commit `Cargo.lock` for binaries, services, examples shipped as products, and workspaces that need reproducible application builds.
- Published libraries may keep a lockfile for CI or omit it according to repository policy; downstream users resolve their own dependency graph.
- Use `--locked` when CI must not mutate resolution.
- Use `--frozen` only when both lockfile changes and network access must be prohibited.
- Review dependency and checksum changes in the lockfile.
- Reproducible dependency resolution does not make build scripts, system libraries, timestamps, generated data, or native toolchains reproducible automatically.

## 8. Optional tools by purpose

Adopt tools deliberately, pin or provision them reproducibly, and define exactly what CI gate they enforce.

### Feature powerset and package matrix: `cargo-hack`

Useful for checking feature combinations, optional dependencies, and per-package matrices. Avoid an exponential powerset when features are numerous; define supported combinations and exclusions.

Example shapes:

```bash
cargo hack check --workspace --feature-powerset --depth 2
cargo hack check --workspace --each-feature
```

### Test runner: `cargo-nextest`

Useful for faster execution, retries, partitioning, profiles, and machine-readable reports. Preserve doctest coverage separately where the selected nextest version or workflow does not run it.

```bash
cargo nextest run --workspace
cargo test --doc --workspace
```

Do not use automatic retries to hide flaky tests; track and fix the root cause.

### Coverage: `cargo-llvm-cov`

Useful for source coverage and CI reports:

```bash
cargo llvm-cov --workspace --all-targets
```

Coverage is diagnostic. Do not optimize tests for a percentage while missing behavior and invariants.

### Dependency policy: `cargo-deny`

Useful for advisories, licenses, bans, sources, and duplicate-version policy. Curate `deny.toml`; do not copy an allowlist without legal and technical review.

```bash
cargo deny check
```

### RustSec advisories: `cargo-audit`

Useful for checking the resolved lockfile against the RustSec Advisory Database:

```bash
cargo audit
```

An advisory match requires triage for reachability, affected versions, mitigation, and update risk. Absence of an advisory is not proof of safety.

### Public API compatibility: `cargo-semver-checks`

Useful for published libraries and explicitly stable APIs:

```bash
cargo semver-checks check-release
```

Review intended breaking changes manually, including semantics, macros, features, MSRV, serialized formats, and behavior not represented in rustdoc metadata.

### Undefined behavior checking: Miri

Useful for unsafe code and suspicious safe abstractions on supported targets:

```bash
cargo +nightly miri test
```

Miri does not support every platform interaction and cannot prove all unsafe code sound. Keep tests focused and respect its documented limitations.

### Concurrency model testing: Loom

Useful for small synchronization components built with Loom-compatible primitives. Model a bounded state space and assert invariants under explored interleavings. Keep production and Loom configurations aligned.

### Fuzzing: `cargo-fuzz`

Useful for parsers, codecs, protocol state machines, unsafe boundaries, and complex input validation. Define corpora, dictionaries, resource limits, crash artifact handling, and continuous execution policy.

### Benchmarks

Use the repository's adopted harness. Criterion, Divan, iai-callgrind, custom load tests, and system benchmarks answer different questions. Keep benchmark configuration and interpretation documented.

### Dependency tree and metadata

Built-in tools often answer dependency questions without installation. Add `--locked` to `cargo metadata` only when the repository has an authoritative lockfile:

```bash
cargo tree -e features
cargo tree -d
cargo metadata --format-version 1
```

Use them to inspect feature activation, duplicates, package ownership, and target-specific resolution.

## 9. Security-sensitive build execution

Running any of the following can execute repository or dependency code:

- `cargo build`, `check`, `test`, `clippy`, `doc`, `run`, and benchmarks.
- Build scripts.
- Procedural macros.
- Test binaries and doctests.
- Tool installation through `cargo install`.

For untrusted code:

- Use an isolated, disposable environment.
- Do not mount host credentials, SSH agents, Docker sockets, cloud metadata, or sensitive home directories.
- Restrict network, filesystem, process, device, and resource access.
- Pin inputs and review what additional tools download or execute.

Static inspection should precede execution, but it cannot fully establish safety.

## 10. Suggested CI stages

Adapt rather than copy:

1. **Metadata and formatting** — fast, no unnecessary matrix.
2. **Stable compile and lint** — affected workspace and supported default configuration.
3. **Tests and doctests** — deterministic test suite.
4. **Feature configurations** — minimal, default, each backend, and valid combinations.
5. **MSRV** — compile or test according to support contract.
6. **Target checks** — supported operating systems and architectures.
7. **Dependency policy** — advisories, licenses, sources, and bans.
8. **API compatibility** — published stable libraries.
9. **Specialized checks** — Miri, fuzzing, Loom, coverage, benchmarks, or integration environments according to risk and cadence.

Keep required checks fast enough to run reliably. Move expensive exploratory checks to scheduled CI only when merge safety remains adequate.

## 11. Change-specific validation matrix

| Change | Add or emphasize |
|---|---|
| Public library API | Docs, doctests, feature docs, SemVer check, downstream example |
| Dependency update | Lockfile review, feature tree, advisories, licenses, MSRV |
| Cargo features | Minimal/default/supported combinations, docs.rs behavior |
| Unsafe code | Focused tests, Miri where supported, safety review, fuzz/property tests |
| Parser or codec | Boundary tests, malformed corpus, fuzzing, size limits |
| Async lifecycle | Cancellation, timeout, shutdown, task leak, backpressure tests |
| Synchronization | Stress tests, Loom where suitable, lock-order review |
| Persistence | Migration, rollback or recovery, mixed-version and failure tests |
| Performance | Representative benchmark, before/after profile, resource impact |
| Target-specific code | Cross-check plus native or emulated behavior test where feasible |
| CLI output | Exit codes, stdout/stderr, pipes, structured output, snapshots selectively |
| Service config | Invalid values, precedence, reload, redaction, startup failure |

## 12. Completion record

For every delivered change, record:

- Toolchain and target used.
- Features and packages checked.
- Exact successful commands.
- Failed commands and whether they reveal a defect or environment limitation.
- Checks skipped and why.
- Remaining unverified contracts.

Never replace this record with “all checks pass” unless the actual supported matrix was executed.
