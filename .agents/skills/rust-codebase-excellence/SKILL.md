---
name: rust-codebase-excellence
description: Use this skill when designing, implementing, reviewing, refactoring, debugging, testing, securing, optimizing, or preparing CI and releases for a small-to-medium Rust crate, binary, service, library, workspace, async system, or FFI boundary. Apply production-grade Rust engineering without cargo-cult rules, speculative abstractions, unnecessary dependencies, or unrelated rewrites. Preserve the repository's public API, MSRV, feature semantics, runtime, targets, and established architecture unless the task explicitly changes them.
license: MIT
compatibility: Stable Rust and Cargo. Honor the repository's edition, rust-version or MSRV, target matrix, feature policy, toolchain files, and existing CI commands.
metadata:
  version: "1.0.0"
  last-reviewed: "2026-08-06"
---

# Rust Codebase Excellence

Produce the smallest coherent change that is correct, maintainable, secure, testable, and appropriate for the repository. Optimize for long-term code quality, not for demonstrating advanced Rust.

## Rule hierarchy

Apply guidance in this order:

1. Explicit user requirements and repository-local instructions.
2. Safety, correctness, data integrity, and externally observable behavior.
3. Public API, wire/storage format, feature, target, and MSRV compatibility.
4. Existing repository architecture and conventions.
5. Simplicity, readability, testability, and operational clarity.
6. Measured performance and resource constraints.
7. Stylistic preferences not enforced by the project.

When rules conflict, follow the higher-ranked rule and state the trade-off. Never silently change a contract.

## Required workflow

### 1. Establish the repository contract

Before editing, inspect the relevant subset of:

- `Cargo.toml` files and workspace layout.
- `Cargo.lock`, `rust-toolchain.toml`, `rustfmt.toml`, and `clippy.toml`.
- CI workflows, build scripts, release configuration, and repository instructions.
- Existing modules adjacent to the change and their tests.
- Public exports, enabled features, supported targets, serialization formats, database schemas, and network protocols affected by the change.

Determine:

- Library, binary, service, proc-macro, build-script, embedded, WASM, or FFI context.
- Edition, declared `rust-version` or other MSRV policy, and stable versus nightly requirements.
- Workspace members, package boundaries, default members, and dependency ownership.
- Supported feature combinations and target platforms.
- Sync versus async design and the chosen runtime.
- Whether the code is public API or internal implementation.
- Whether `unsafe`, untrusted input, secrets, persistence, concurrency, or compatibility boundaries are involved.

Do not execute Cargo commands in an untrusted repository until build scripts, proc macros, and dependencies can run inside an appropriate sandbox. Cargo compilation may execute code.

### 2. Load only relevant references

Read each selected file before deciding on the implementation. For repository-wide audits, read the review playbook and every applicable domain reference; load the source index only when guidance must be verified or maintained.

| Trigger | Reference |
|---|---|
| Workspace layout, modules versus crates, Cargo features, MSRV, public API, SemVer | [architecture-cargo-api.md](references/architecture-cargo-api.md) |
| Ownership, borrowing, types, traits, error design, panics, invariants | [idioms-types-errors.md](references/idioms-types-errors.md) |
| Async code, threads, tasks, locks, channels, cancellation, shutdown | [async-concurrency.md](references/async-concurrency.md) |
| Tests, linting, feature matrices, CI validation, coverage | [testing-verification.md](references/testing-verification.md) |
| Dependencies, untrusted input, secrets, `unsafe`, FFI, supply chain | [security-dependencies-unsafe.md](references/security-dependencies-unsafe.md) |
| Latency, throughput, allocations, memory, binary size, compile time | [performance-resource-use.md](references/performance-resource-use.md) |
| Documentation, tracing, metrics, CLI/service behavior, operations | [documentation-observability-operations.md](references/documentation-observability-operations.md) |
| Patch review, repository audit, migration review, finding severity | [review-playbooks.md](references/review-playbooks.md) |
| Concrete Cargo lint and command baselines, optional ecosystem tools | [tooling-baseline.md](references/tooling-baseline.md) |
| Updating this skill or verifying disputed version/tool behavior | [sources.md](references/sources.md) |

### 3. Plan the change around contracts

Identify the behavior that must remain true and the behavior that must change. Check failure paths, cleanup, compatibility, and test seams before coding.

Prefer:

- A focused implementation over a framework.
- Existing abstractions over parallel abstractions.
- A module over a new crate unless a real package boundary exists.
- Concrete types over traits or generics with only one credible implementation.
- An enum over typestate when runtime state is simpler and sufficient.
- Standard library facilities over a dependency when the implementation remains clear and correct.
- A dependency over custom security-sensitive or protocol code when the dependency is established, appropriately scoped, and approved.

Do not mix unrelated cleanup into the patch. Separate prerequisite refactors only when they materially reduce risk and keep them behavior-preserving.

### 4. Implement defensively

#### Correctness and invariants

- Make invalid states hard to construct where the benefit exceeds the complexity.
- Validate external input at the boundary; keep validated internal representations.
- Use checked, saturating, or explicitly wrapping arithmetic according to domain semantics. Never rely accidentally on debug-versus-release overflow behavior.
- Treat integer narrowing, signedness changes, sizes, offsets, timestamps, units, encodings, paths, and identifiers as boundary decisions.
- Keep resource ownership explicit. Ensure files, sockets, locks, tasks, temporary state, and transactions are released or rolled back on every path.
- Preserve atomicity and idempotency where retries or partial failures are possible.

#### Ownership and API shape

- Borrow when the callee only observes data for the duration of the call. Own data when it must be retained, moved, queued, spawned, cached, or independently mutated.
- Choose ownership from semantics, not fixed byte-size rules.
- Avoid clones that obscure ownership or occur on hot paths; accept a clear, cheap clone when it is the simplest correct design.
- Use `&str`, `&[T]`, `&Path`, and related borrowed forms when callers naturally already own the data. Do not contort lifetimes merely to avoid a small allocation.
- Use `Cow` only when both borrowed and occasionally owned results are natural parts of the API.
- Introduce newtypes for validated values, units, sensitive identifiers, or otherwise confusable primitives when they eliminate real mistakes.
- Keep visibility minimal. Expanding `pub` is an API decision, not a convenience.

#### Errors and panics

- Return `Result` for failures callers may handle, classify, retry, report, or convert.
- Reserve panics for bugs, violated internal invariants, impossible states after validation, and intentionally process-fatal startup assumptions.
- `unwrap()` or `expect()` is acceptable when the proof is local and obvious, in tests, or at a deliberate fatal boundary. Otherwise propagate or handle the error.
- Prefer `expect()` over `unwrap()` when the message can state the invariant that was violated, not merely restate the operation.
- Preserve causal chains and add context at abstraction boundaries.
- Use stable typed errors for library or domain boundaries. Application entry points may use report-oriented errors when callers do not need programmatic matching.
- Do not expose secrets, credentials, raw sensitive payloads, or unnecessary internal details in errors.

#### Abstraction discipline

- Add a trait for multiple implementations, a meaningful behavioral boundary, dependency inversion, object-safe runtime selection, or a proven test seam—not solely to wrap one concrete type.
- Choose generics for compile-time polymorphism and `dyn Trait` for runtime polymorphism, heterogeneous values, reduced monomorphization, or plugin-like boundaries. Neither is universally superior.
- Use builders when construction has many optional fields or staged validation. Prefer constructors and explicit parameter objects for simpler cases.
- Use typestate only when compile-time protocol enforcement materially prevents misuse and the state space is stable. Do not retain the same invalid runtime representation behind phantom states.

#### Async and concurrency

- Do not introduce async merely for style; use it when the surrounding architecture or concurrency requirements justify it.
- Do not block an async executor thread. Move blocking or CPU-heavy work to the runtime's designated mechanism or a bounded worker pool.
- Do not hold a synchronous lock across `.await`. Holding an async lock across `.await` must be deliberate, short, and appropriate for the protected resource.
- Give every spawned task an owner, completion path, error path, cancellation policy, and shutdown behavior. Avoid detached fire-and-forget tasks.
- Bound queues, concurrency, retries, and memory growth. Define backpressure instead of assuming producers and consumers remain balanced.
- Review cancellation at every `.await` in operations that mutate state or perform multi-step I/O.

#### `unsafe`

- Prefer safe Rust. Do not use `unsafe` to bypass an inconvenient ownership design.
- Keep each unsafe block as small as practical and place it behind a safe abstraction whose invariants can be reviewed.
- Add a precise `SAFETY:` explanation for every unsafe block and unsafe trait implementation.
- Document `# Safety` for every public unsafe function or trait.
- Validate pointers, lengths, alignment, initialization, aliasing, lifetimes, provenance, thread guarantees, ABI, ownership transfer, and unwind behavior as applicable.

### 5. Validate using the repository's real matrix

Use repository-provided commands first. Do not install tools or change toolchain versions without approval.

At minimum, when applicable and supported by the repository:

1. Format check.
2. Compilation or `cargo check` for affected packages and targets.
3. Clippy with warnings treated as errors in CI scope.
4. Focused tests for the changed behavior.
5. Broader package or workspace tests.
6. Doc tests and documentation build for public APIs.
7. Supported feature combinations, not blindly `--all-features`.
8. MSRV, platform, security, SemVer, Miri, fuzz, model-concurrency, or performance checks when the change touches those contracts.

Do not claim a command passed unless it was executed successfully. If a check cannot run, state the exact reason and what remains unverified.

### 6. Review the final diff

Before completing:

- Re-read the changed code without relying on intent.
- Trace success, error, cancellation, panic, and cleanup paths.
- Check public API, feature, target, MSRV, serialization, storage, and operational compatibility.
- Remove dead code, stale comments, debug output, broad lint suppressions, accidental clones, unnecessary allocations, and unrelated edits introduced by the patch.
- Confirm tests fail for the old bug or missing behavior and pass for the new behavior when practical.
- Confirm the solution is no more abstract than the problem requires.

## Lint policy

- Fix owned-code warnings instead of silencing them.
- Keep suppressions at the narrowest possible scope and explain why the lint does not apply.
- Use `#[expect(...)]` when the repository MSRV supports it and the lint is intentionally expected; otherwise use a justified `#[allow(...)]`.
- Do not enable all of `clippy::pedantic` or `clippy::restriction` as errors without curating the resulting policy for the codebase.
- Prefer CI command-line `-D warnings` over `#![deny(warnings)]` in published libraries, because new compiler lints should not unexpectedly break downstream builds.
- Let `rustfmt` decide formatting. Do not spend review effort on formatting already enforced by tooling.

## Testing policy

- Test externally meaningful behavior and invariants, not line-by-line implementation details.
- Each test should have one coherent reason to fail; multiple related assertions are valid.
- Cover success, boundary, malformed-input, error, retry, cancellation, and regression cases relevant to the change.
- Keep tests deterministic and parallel-safe. Avoid wall-clock sleeps, shared global mutation, ambient network access, and order dependence.
- Prefer a fake clock, seeded randomness, temporary isolated resources, and explicit dependency seams where needed.
- Use snapshots for complex stable output only when diffs are reviewed and assertions on critical semantics remain explicit.
- Treat coverage as a diagnostic, not a quality target. High coverage does not replace meaningful assertions.

## Dependency policy

- Add no dependency merely to save a few clear lines of code.
- Add no custom implementation of cryptography, parsers, codecs, synchronization primitives, or security-sensitive protocols merely to avoid a dependency.
- Inspect default features, transitive impact, MSRV, maintenance, license, source, build scripts, proc macros, and security history before adding a dependency.
- Keep dependency features minimal but do not disable defaults blindly.
- Preserve lockfile and update policy. Dependency updates must be intentional and separately reviewable when practical.

## Prohibited default actions

Unless explicitly required, do not:

- Raise the MSRV or migrate editions.
- Change the async runtime, allocator, panic strategy, serialization format, protocol, database schema, or public error variants.
- Split the workspace into more crates or merge crates.
- Add traits, typestate, macros, `Arc<Mutex<_>>`, interior mutability, or `unsafe` preemptively.
- Run `--all-features` when features are mutually exclusive or the repository does not support that combination.
- Add broad `allow`, `expect`, or dead-code attributes to make CI green.
- Replace clear loops with iterator chains or clear iterator chains with loops solely for style.
- Optimize without a stated bottleneck or evidence.
- Rewrite adjacent code that is not required for the task.

## Completion report

Report:

1. What changed and why.
2. Contracts intentionally preserved or intentionally changed.
3. Tests and validation commands actually executed.
4. Any remaining risk, uncertainty, platform gap, feature gap, or unverified assumption.

For code reviews, use the severity and evidence rules in [review-playbooks.md](references/review-playbooks.md). Report no finding without a concrete failure mode or maintainability cost.
