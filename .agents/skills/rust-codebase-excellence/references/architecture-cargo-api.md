# Architecture, Cargo, and Public API

Read this reference for workspace structure, module or crate boundaries, Cargo configuration, features, MSRV, target support, publishing, and public API compatibility.

## 1. Start from the actual product boundary

Classify every package before changing structure:

- **Application or service:** optimized for reproducible deployment, operational behavior, and controlled upgrades.
- **Library:** optimized for downstream compatibility, small public surface, feature composability, and clear error contracts.
- **Proc macro:** executes in the compiler process and is a distinct package boundary.
- **Build helper or `build.rs`:** host-side executable code with cross-compilation implications.
- **FFI crate:** ABI and safety boundary that often deserves isolation.
- **Embedded or `no_std`:** allocator, panic, target, and dependency constraints are first-class contracts.
- **WASM:** host bindings, size, feature availability, and runtime assumptions are contracts.

Do not apply library guidance to an internal application mechanically, or application shortcuts to a public library.

## 2. Module versus crate

Prefer a **module** when code:

- Is released, versioned, and deployed with its parent package.
- Shares the same dependency policy and target matrix.
- Needs privacy boundaries rather than independent compilation.
- Has no credible independent consumer.
- Would require cyclic or awkward package APIs if extracted.

Create a **crate** only when at least one durable boundary exists:

- Independent public API or reuse by multiple packages.
- A proc-macro package, which Cargo requires separately.
- FFI, platform-specific, or unsafe code that benefits from dependency and audit isolation.
- A dependency boundary that materially reduces unwanted transitive dependencies or target incompatibility.
- Independent versioning, publishing, ownership, or release cadence.
- A stable plugin or protocol boundary.
- Measured build-graph benefits that outweigh API and coordination cost.

A directory being large is not sufficient reason to create a crate. Split modules by cohesion and visibility first.

### Crate extraction gate

Before extracting, answer:

1. What contract will the new crate own?
2. Who consumes it independently?
3. Which dependencies or targets become better isolated?
4. What public API and SemVer burden is introduced?
5. Could a private module solve the same problem?

If the answers are weak, keep the code in one package.

### Source and module hygiene

- Organize modules around cohesive responsibilities and owned invariants, not arbitrary line counts or one-type-per-file rules.
- Keep `lib.rs` and `main.rs` focused on public composition, startup, and wiring rather than accumulating implementation logic.
- Avoid catch-all modules named `utils`, `common`, `helpers`, or `misc` unless they have a precise, stable responsibility.
- Keep domain logic independent from framework, transport, persistence, and process-global state where this produces a real testable boundary.
- Use module privacy to enforce direction and invariants; do not replace a clear private module boundary with a public crate API.
- Keep re-exports deliberate. A prelude or glob export is appropriate only when the curated namespace is itself an intentional API.
- Prefer explicit imports in production code when they make dependencies and name origins clear; use glob imports only where the namespace is deliberately controlled.
- Co-locate small unit tests with private logic. Put downstream-facing, cross-module, binary, and environment boundary tests in appropriate integration targets.
- Isolate generated, vendored, platform-specific, and unsafe implementation code so normal review and lint scope remains clear.
- Split a large file when cohesion, navigation, parallel ownership, or conditional compilation improves—not to satisfy an arbitrary maximum.

## 3. Workspace discipline

For workspaces:

- Keep the root manifest explicit about `members`, `default-members`, and `resolver`.
- Centralize shared package metadata and dependency versions only when members genuinely share policy.
- Remember that workspace dependency features are additive; centralization can accidentally enable more features.
- Use `[workspace.lints]` with `[lints] workspace = true` in each member when the MSRV supports workspace lint inheritance.
- Keep package-specific features and dependencies in the package that owns them.
- Avoid a single “common” crate becoming an unbounded dependency sink. Prefer cohesive domain crates or private modules.
- Keep dependency direction acyclic and intentional. Higher-level application crates depend on lower-level domain or infrastructure crates, not the reverse.

A workspace is an organizational and build unit, not an excuse to fragment the design.

## 4. Edition, toolchain, and MSRV

Treat these as explicit compatibility contracts:

- The Rust edition controls language behavior; it is not the compiler version.
- `rust-version` declares the supported minimum toolchain for a package.
- `rust-toolchain.toml` pins or selects the development toolchain; it does not replace a library's MSRV policy.
- Nightly use must be visible, justified, pinned where reproducibility matters, and isolated from stable consumers where possible.

Rules:

- Never raise MSRV or migrate edition as incidental cleanup.
- Do not use a language or library feature newer than the declared MSRV.
- When introducing `#[expect(...)]`, confirm MSRV is at least Rust 1.81; otherwise use a narrow justified `#[allow(...)]`.
- Test the actual MSRV in CI when the project promises one. Building on stable alone does not verify MSRV.
- For workspaces with different MSRVs, verify each published package independently; dependency unification can hide incompatibilities.
- Treat an MSRV increase as a user-visible release decision and document it.

## 5. Cargo features

Features are part of a library's public API and a build contract for applications.

### Design rules

- Make features **additive** whenever possible: enabling a feature should add capability, not disable or replace another capability.
- Avoid mutually exclusive features. Prefer runtime selection, separate crates, or one feature with configuration when feasible.
- Keep default features conservative and useful. Removing a default feature can be SemVer-incompatible.
- Use `dep:name` to avoid exposing internal optional dependency names as public features when supported by the MSRV.
- Do not create a feature for every internal implementation detail.
- Document feature effects, dependencies, target restrictions, and incompatible combinations.
- Do not assume optional dependencies are absent merely because one edge disables default features; Cargo feature unification may enable them elsewhere.

### Verification matrix

Define supported combinations deliberately. A typical library matrix may include:

1. Default features.
2. `--no-default-features` if supported.
3. Each public leaf feature individually where meaningful.
4. Supported named combinations.
5. `--all-features` only when every feature is designed to coexist.
6. Target-specific combinations on their actual targets.

Do not attempt the full powerset unless the feature count and semantics make it useful. Prefer a curated matrix plus `cargo-hack` when the repository already adopts it.

### Compile-time conflict handling

If mutually exclusive features are unavoidable:

- Fail early with a clear `compile_error!`.
- Document valid selections.
- Exclude invalid combinations from generic `--all-features` CI.
- Test each valid combination explicitly.

## 6. Dependency boundaries

Before adding or moving a dependency, inspect:

- Direct and transitive features.
- Target-specific activation.
- MSRV and edition requirements.
- Build scripts and proc macros.
- License and source policy.
- Native system dependencies and cross-compilation impact.
- Maintenance and security status.
- Whether it leaks into public types or trait bounds.

A dependency used in a public signature can become part of the compatibility surface. Prefer re-exporting only when users genuinely need the exact type and the commitment is intentional.

### Lockfile policy

- Commit `Cargo.lock` for binaries, services, deployed applications, and application workspaces.
- For library-only repositories, follow the repository's policy. A committed lockfile stabilizes local and CI resolution but does not constrain downstream consumers.
- Public libraries must test their declared dependency ranges, not only one locked resolution.
- Use `--locked` in reproducibility-sensitive CI only when a lockfile is intentionally authoritative.
- Keep dependency updates focused and review lockfile changes for unexpected additions, duplicate versions, native code, build scripts, or source changes.

## 7. Public API design

Public API includes more than `pub fn`:

- Public modules, types, traits, methods, fields, variants, constants, macros, and re-exports.
- Trait implementations, including auto traits such as `Send`, `Sync`, and `Unpin`.
- Generic bounds, lifetimes, associated types, and object-safety behavior.
- Error variants and source types if callers match them.
- Feature names and defaults.
- Serialization formats, command-line interface, configuration keys, environment variables, exit codes, and protocol behavior.
- Panic behavior where documented or relied upon.

### Surface-area rules

- Default to private, then `pub(crate)`, then `pub` only when external use is intended.
- Expose behavior rather than fields when invariants matter.
- Avoid exposing implementation-specific dependency types without intent.
- Return concrete types when callers benefit from their API; use `impl Trait` when hiding the implementation is part of the contract.
- Avoid generic parameters that exist only to accommodate hypothetical future implementations.
- Use sealed traits when external implementations would prevent safe evolution.
- Use `#[non_exhaustive]` when downstream exhaustive construction or matching would block planned evolution, but understand the ergonomic cost.
- Add `#[must_use]` when silently discarding a value is likely a bug, not on every return type.
- Implement standard traits when semantics are unsurprising: `Debug`, `Clone`, `Default`, `From`, `TryFrom`, `AsRef`, `Borrow`, `Iterator`, `Error`, and others as appropriate.
- Do not implement `Deref` merely to simulate inheritance or forward an API.

### Constructors and configuration

- Use `new` for the obvious primary constructor.
- Use named constructors when construction semantics differ.
- Use a parameter struct when many required arguments would be ambiguous.
- Use a builder when many options are optional, staged validation is useful, or forward-compatible construction matters.
- Validate once and return a fully valid type. Avoid “half initialized” public objects.

## 8. SemVer review

Before publishing a library release, inspect changes to:

- Removed, renamed, moved, or newly private items.
- Function signatures, generic bounds, lifetimes, and trait object compatibility.
- Enum variants and public struct fields.
- Trait methods, required methods, associated items, and implementability.
- Implemented or removed auto traits and blanket implementations.
- Feature names, defaults, and dependency feature exposure.
- MSRV.
- Panic, error, serialization, and behavioral contracts.

Use `cargo-semver-checks` when the project already provides it or approves the tool, but do not treat automated output as complete. Behavioral and wire-format compatibility still require human review.

## 9. Build scripts and proc macros

Build scripts and proc macros execute host code during compilation.

- Keep them minimal, deterministic, and free from undeclared network access.
- Emit precise rerun directives so unrelated file changes do not rebuild everything.
- Distinguish host and target configuration during cross-compilation.
- Place generated files in `OUT_DIR`; do not rewrite tracked source during normal builds.
- Validate generated inputs and provide actionable failures.
- Pin external generators or document required versions.
- Treat new proc-macro and build dependencies as elevated supply-chain changes.

## 10. Macros and generated code

Prefer functions, traits, generics, and ordinary modules when they express the abstraction clearly. Use macros when syntax generation, compile-time structure, or repetition across otherwise unexpressible item shapes provides real value.

For declarative macros:

- Keep accepted syntax small and unsurprising.
- Use hygienic paths such as `$crate` for items owned by the defining crate.
- Avoid accidental name capture and undocumented evaluation order.
- Do not evaluate user expressions more than the documented number of times.
- Preserve visibility and error locality; public macros are public API.
- Test expansion in downstream-style integration crates and under relevant features.

For procedural macros:

- Treat input as untrusted compiler input: return useful span-aware diagnostics instead of panicking.
- Keep expansion deterministic and free from undeclared network, filesystem, environment, and time dependencies.
- Minimize generated public surface and fully qualify generated paths where hygiene requires it.
- Test successful expansion, compile failures, generics, attributes, visibility, renamed dependencies, and malformed input.
- Consider compile-time and dependency cost before adding a proc macro for convenience.

For generated code:

- Keep the generator, inputs, version, and regeneration command reproducible.
- Generate into `OUT_DIR` for build artifacts; commit generated source only when repository policy has a concrete distribution or bootstrapping reason.
- Make generated failures point back to useful source input when possible.
- Never hand-edit generated output without updating its source of truth.

## 11. Target and platform support

- Put platform-specific dependencies under target-specific Cargo sections when possible.
- Keep `cfg` expressions centralized or wrapped in modules instead of scattering platform behavior throughout business logic.
- Use Cargo's checked configuration support and declare custom `cfg` values emitted by build scripts.
- Test on the actual target when ABI, filesystem, path encoding, endianness, atomics, threading, or native libraries matter.
- Do not infer Windows behavior from Unix tests or WASM behavior from native tests.

## 12. Packaging, release, and distribution

Treat the shipped package or artifact—not only the workspace checkout—as the release input.

Before a release, as applicable:

- Inspect the packaged file list and exclude secrets, local state, oversized fixtures, accidental generated output, and unrelated artifacts.
- Verify the crate from the packaged source so workspace-only files, path dependencies, ignored files, or generated assumptions do not hide a broken release.
- Confirm package metadata, license files, README, repository links, feature documentation, and declared `rust-version` are accurate.
- Review version, changelog, SemVer impact, feature defaults, dependency ranges, and publish order for related workspace crates.
- Publish or build artifacts from a reviewed source revision through repository-approved release tooling. A dirty-tree override may be useful for a local packaging check but is not release provenance.
- Test installation or downstream consumption in a clean environment when the distribution path can differ from workspace builds.
- For binaries, record target triples, profile, system-library assumptions, configuration compatibility, checksums, and provenance or signing requirements.
- Generate SBOM, attestations, or reproducibility evidence when the deployment, customer, or compliance contract requires them.
- Define rollback, yanking, deprecation, and security-advisory handling before an incident requires them.

Do not bypass package verification merely to make a release command succeed. If verification requires network or executes build-time code, run it under the same trust and isolation rules as other Cargo execution.

## Architecture completion checklist

- Package and module boundaries reflect real ownership.
- No accidental MSRV, edition, resolver, or target change.
- Features remain additive or invalid combinations are explicit.
- Public surface is minimal and documented.
- Dependency and lockfile changes are intentional.
- SemVer-sensitive changes are identified.
- Build scripts and proc macros remain deterministic and reviewable.
- Packaged source and release artifacts match the reviewed contracts.
