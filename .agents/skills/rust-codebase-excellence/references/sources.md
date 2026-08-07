# Primary Sources and Maintenance Notes

Last reviewed: 2026-08-06.

This skill intentionally contains derived engineering guidance rather than copying external prose. Re-check these primary sources when updating rules that depend on Rust, Cargo, tooling, or ecosystem behavior.

## Agent Skills format

- Agent Skills specification: https://agentskills.io/specification
- Agent Skills integration and progressive disclosure: https://agentskills.io/client-implementation/adding-skills-support
- Agent Skills authoring guidance: https://agentskills.io/skill-creation/best-practices
- Reference validator: https://github.com/agentskills/agentskills/tree/main/skills-ref

## Rust language and standard library

- The Rust Reference: https://doc.rust-lang.org/reference/
- The Rustonomicon: https://doc.rust-lang.org/nomicon/
- Standard library documentation: https://doc.rust-lang.org/std/
- `Send`: https://doc.rust-lang.org/std/marker/trait.Send.html
- `Sync`: https://doc.rust-lang.org/std/marker/trait.Sync.html
- Error handling module: https://doc.rust-lang.org/std/error/
- `Pin`: https://doc.rust-lang.org/std/pin/
- `PhantomData`: https://doc.rust-lang.org/std/marker/struct.PhantomData.html
- `Drop`: https://doc.rust-lang.org/std/ops/trait.Drop.html
- Macros by example: https://doc.rust-lang.org/reference/macros-by-example.html
- Procedural macros: https://doc.rust-lang.org/reference/procedural-macros.html
- Rust 1.81 release notes (`#[expect]` stabilization): https://blog.rust-lang.org/2024/09/05/Rust-1.81.0/

## Cargo

- Cargo reference: https://doc.rust-lang.org/cargo/reference/
- Workspaces: https://doc.rust-lang.org/cargo/reference/workspaces.html
- Features: https://doc.rust-lang.org/cargo/reference/features.html
- Resolver: https://doc.rust-lang.org/cargo/reference/resolver.html
- `rust-version`: https://doc.rust-lang.org/cargo/reference/rust-version.html
- Build scripts: https://doc.rust-lang.org/cargo/reference/build-scripts.html
- Profiles: https://doc.rust-lang.org/cargo/reference/profiles.html
- Package and publish: https://doc.rust-lang.org/cargo/reference/publishing.html
- Package layout and inclusion: https://doc.rust-lang.org/cargo/commands/cargo-package.html
- Environment variables: https://doc.rust-lang.org/cargo/reference/environment-variables.html

## API and documentation guidance

- Rust API Guidelines: https://rust-lang.github.io/api-guidelines/
- rustdoc book: https://doc.rust-lang.org/rustdoc/
- SemVer compatibility guide: https://doc.rust-lang.org/cargo/reference/semver.html

## Async and concurrency

- Async Rust book: https://rust-lang.github.io/async-book/
- Async cancellation: https://rust-lang.github.io/async-book/part-guide/more-async-await.html
- Tokio documentation: https://docs.rs/tokio/latest/tokio/
- Tokio shared state: https://tokio.rs/tokio/tutorial/shared-state
- Loom: https://docs.rs/loom/latest/loom/

## Verification and supply chain

- Clippy documentation: https://doc.rust-lang.org/clippy/
- Miri: https://github.com/rust-lang/miri
- Rust Fuzz Book: https://rust-fuzz.github.io/book/
- cargo-fuzz: https://github.com/rust-fuzz/cargo-fuzz
- RustSec Advisory Database: https://rustsec.org/
- cargo-audit: https://github.com/rustsec/rustsec/tree/main/cargo-audit
- cargo-deny: https://embarkstudios.github.io/cargo-deny/
- cargo-semver-checks: https://github.com/obi1kenobi/cargo-semver-checks
- cargo-hack: https://github.com/taiki-e/cargo-hack
- cargo-nextest: https://nexte.st/
- cargo-llvm-cov: https://github.com/taiki-e/cargo-llvm-cov

## Security and unsafe-code review

- Rust unsafe code guidelines repository: https://github.com/rust-lang/unsafe-code-guidelines
- Rustonomicon FFI chapter: https://doc.rust-lang.org/nomicon/ffi.html

## Maintenance policy

When updating this skill:

1. Preserve repository-local policy precedence and the non-dogmatic stance.
2. Verify stabilization versions against official Rust release notes.
3. Verify Cargo behavior against the current Cargo reference.
4. Keep optional tool commands aligned with their official documentation.
5. Remove stale ecosystem recommendations rather than accumulating alternatives.
6. Re-run skill format validation and the scenario evaluations in `EVALUATION.md`.
7. Update `metadata.version` and `metadata.last-reviewed` in `SKILL.md`.
