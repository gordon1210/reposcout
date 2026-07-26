# Development

← [Documentation index](README.md)

RepoScout is a Rust 2024 CLI with a pnpm frontend workspace.

## Requirements

- Rust via `rustup`
- a C compiler for vendored libgit2 and tree-sitter grammars
- pnpm `11.13.1` for frontend work

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
pnpm build:web
pnpm test:web
pnpm build:landing
```

Live development commands are documented in [Daemon and web dashboard](daemon-and-web.md).

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

- Apple Silicon macOS;
- Intel macOS; and
- x86-64 GNU/Linux.

Archives contain the binary, README, changelog, MIT license, and third-party notices. The workflow
also publishes individual checksums, an aggregate checksum file, source archive, manifest, and
shell installer.

The initial distribution channel is GitHub Releases. crates.io and Homebrew are deliberately
deferred.
