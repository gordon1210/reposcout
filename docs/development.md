# Development

← [Documentation index](README.md)

RepoScout is a Rust 2024 CLI with a pnpm frontend workspace.

## Requirements

- Rust via `rustup`
- a C compiler for vendored libgit2 and tree-sitter grammars
- pnpm `11.18.0` for frontend work

`cmake` is not required.

## Rust commands

```sh
cargo build
cargo build --release
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt
cargo run -- -f json .
```

The repository configures serialized integration tests and bounded RepoScout child processes.
Bare `cargo test` is the intended full-suite command.

## Frontend commands

```sh
pnpm install
pnpm lint:frontend
pnpm lint:fix:frontend
pnpm build:web
pnpm test:web
pnpm build:landing
```

Live development commands are documented in [Daemon and web dashboard](daemon-and-web.md).
The shared config lives in `packages/eslint-config`; each app keeps only a small local adapter.
Imported shadcn primitives under `apps/web/src/components/ui/` are intentionally excluded from
linting and must not be edited by hand.
Production modules are capped at cyclomatic complexity 20 and 900 non-blank, non-comment lines.
Tests retain correctness and formatting checks while opting out of size, complexity, strict
assertion, and development-only React rules.

## Tests and fixtures

Integration tests live in `tests/` and run the compiled CLI against bounded fixtures.

`tests/fixtures/dup_languages.toml` is the canonical 31-format duplication corpus. Every format has
multi-line exact and Type-2 examples tested through both the frozen detector adapters and an
end-to-end `reposcout dup` scan.

The shared command helper applies the same global worker limit to every CLI child. New integration
tests should use bounded synthetic repositories rather than scanning this repository.

## Architecture

The stable serialized contract lives in [`src/model.rs`](../src/model.rs). Analyzers write those
types, the scanner aggregates them, and reporters consume them:

```text
CLI
  → discovery
  → per-file analyzers and cache
  → cross-file duplication / Git / graph queries
  → aggregate + canonical findings
  → table / JSON / Markdown / SARIF / NDJSON / graph renderers
```

Important module boundaries:

| Area | Files |
|---|---|
| CLI and dispatch | `src/cli.rs`, `src/main.rs` |
| Stable data contract | `src/model.rs` |
| Discovery and source snapshots | `src/walk.rs`, `src/snapshot.rs` |
| Scan orchestration | `src/scan.rs` |
| Per-file metrics | `src/metrics/` |
| Duplication | `src/dup/` |
| Git history and diffs | `src/git.rs` |
| Graph, impact, and symbols | `src/graph.rs`, `src/graph/` |
| Context and task queries | `src/context.rs`, `src/query.rs`, `src/explain.rs` |
| Output formats | `src/report/` |
| Dashboard and landing page | `apps/web/`, `apps/landing/` |
| Dashboard composition | `apps/web/src/components/dashboard.tsx`, `dashboard-report.tsx` |
| Graph controller and views | `apps/web/src/components/repository-graph*.ts(x)` |
| Graph domain projections | `apps/web/src/lib/graph-*.ts` |
| Shared frontend lint rules | `packages/eslint-config/` |

Read [`AGENTS.md`](../AGENTS.md) before changing code. It documents frozen interfaces, schema/cache
versioning rules, metric semantics, resource guardrails, and repository-specific validation.

## Contract rules

- JSON changes are additive unless `SCHEMA_VERSION` is bumped.
- New optional/list/bool fields use Serde defaults for older reports and caches.
- Per-file analysis changes require an `ANALYZER_VERSION` bump.
- `scan.rs` remains the orchestrator for analyzer output.
- The public exact and fuzzy detector adapter signatures stay frozen.
- Cross-cutting duplication behavior belongs in `src/dup/mod.rs`.

## Release builds

Release packaging uses cargo-dist and the configuration in
[`dist-workspace.toml`](../dist-workspace.toml). A signed semantic-version tag triggers
`.github/workflows/release.yml`, which builds:

- Apple Silicon macOS; and
- x86-64 GNU/Linux.

Archives contain the binary, README, changelog, MIT license, and third-party notices. The workflow
also publishes individual checksums, an aggregate checksum file, source archive, manifest, and
shell installer. The generated installer is hardened to fail closed unless it can verify SHA-256,
release actions are pinned to immutable commits, and every published artifact receives GitHub
build-provenance attestation. Release binaries embed auditable dependency metadata, and each
release includes a CycloneDX software bill of materials.

The release workflow is intentionally hand-hardened and `allow-dirty = ["ci"]` prevents
cargo-dist from overwriting its least-privilege permissions, immutable action pins, installer
verification patch, and attestation step. Review and refresh those customizations deliberately
when upgrading cargo-dist. Superseded pull-request runs are cancelled, and release tools are
installed from pinned, checksum-verified upstream installer scripts rather than compiled afresh
on every runner.

The initial distribution channel is GitHub Releases. crates.io and Homebrew are deliberately
deferred.
