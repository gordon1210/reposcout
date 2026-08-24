# Agent validation and repository safety

This is a normative extension of the root [`AGENTS.md`](../../AGENTS.md). Read it completely before
changing dependencies, running builds or tests, touching frontend code, or interacting with any
long-running process. The root instructions remain in force.

## Development binary

A global `reposcoutdev` command is installed as a symlink on this machine:

```text
~/.local/bin/reposcoutdev  ->  <repo>/target/release/reposcout
```

The public `reposcout` command is reserved for an installed release and must not point into this
working tree. Because `reposcoutdev` targets the release build, rebuild it after **every code
change**:

```sh
cargo build --release
```

If the symlink is missing or broken, for example after `cargo clean` or moving the repository,
recreate and verify it:

```sh
mkdir -p ~/.local/bin
ln -sf "$(pwd)/target/release/reposcout" ~/.local/bin/reposcoutdev
reposcoutdev --version
```

Do not rely on `target/debug`; a debug-only build does not refresh `reposcoutdev`.

## Long-running process ownership

- Never start, stop, restart, signal, kill, or otherwise control `reposcout daemon`, a frontend
  development server, Vite, a file watcher, a browser, or any other long-running process unless the
  user explicitly authorizes that interaction in the current task.
- Treat every pre-existing or unknown process as user-owned. A listening port, matching command
  name, repository path, or apparent relationship to the task does not establish agent ownership.
- When the user explicitly authorizes a temporary process, record its exact session or PID. Cleanup
  may target only that recorded process; never use broad process-name or port-based termination.
- Do not start daemons or frontends for routine validation. Prefer one-shot tests and production
  builds. If live validation is necessary but unauthorized, report that it was not performed.

## Resource-safety guardrails

RepoScout integration tests launch the compiled CLI. `.cargo/config.toml` sets
`RUST_TEST_THREADS=1`, the shared command helper uses `tests/fixtures/test-global.toml` to cap each
RepoScout child at two worker threads, and tests use bounded fixtures. Bare `cargo test` is the
intended full-suite command.

- Do not override `RUST_TEST_THREADS`, pass a larger `--test-threads` value, or bypass
  `tests/support/command.rs` during routine validation. A direct CLI child needed for a process-I/O
  test must use the same `test_global_config()` path.
- Keep integration-test targets synthetic and bounded. Do not scan `CARGO_MANIFEST_DIR` merely to
  obtain a large or representative report.
- Run at most one build, test suite, benchmark, or RepoScout scan at a time. Do not parallelize
  resource-intensive validation in shell jobs, tool calls, or subagents.
- Do not run an unbounded RepoScout scan or benchmark against a large external repository without
  explicit user authorization. Prefer focused targets and bounded history.
- Record the exact session/PID for every manually authorized resource-intensive command and
  monitor it. If it unexpectedly exceeds 180 seconds, 1 GiB RSS, or one RepoScout CLI child, stop
  only that recorded agent-owned session and report what happened.
- If a guardrail stops broad validation, run the smallest relevant targeted checks and state
  plainly what was not run.

## Toolchain

- Rust edition 2024 is installed through `rustup`. If `cargo` is unavailable in a fresh shell,
  load it with `source "$HOME/.cargo/env"`.
- A C compiler is required for vendored `libgit2` and the tree-sitter grammars; `cmake` is not.
- Frontend packages use the root pnpm workspace. The authoritative pnpm version is the
  `packageManager` field in `package.json`; do not duplicate that pin in documentation.

## Common commands

```sh
cargo build                 # debug build
cargo build --release       # release build; refreshes reposcoutdev
cargo test                  # full suite; serialized by repository configuration
cargo test <FILTER>         # targeted test with the same safe defaults
cargo clippy --all-targets -- -D warnings
cargo fmt
cargo run -- -f json .
pnpm lint:frontend
pnpm lint:fix:frontend
pnpm build:web
pnpm test:web
pnpm build:landing
./scripts/reposcout-skill.sh sync
./scripts/reposcout-skill.sh check
```

## Dependency and supply-chain maintenance

- Reproduce dependency alerts and trace the complete dependency path before changing manifests or
  lockfiles. For transitive findings, first use the package manager's dedicated lockfile/security
  update path (for pnpm, `pnpm audit --fix=update`) and inspect its diff. A targeted
  direct-dependency update doing nothing shows that the package is transitive; it does not justify
  an override.
- Overrides, resolutions, direct transitive pins, audit ignores, and release-age exclusions are
  last-resort policy changes. Use one only after proving normal constraints cannot select a fixed
  version. Keep it narrow and document the reason and exact removal condition. Never accept an
  unrelated broad refresh merely to clear one advisory.
- Before retaining an exception such as `minimumReleaseAgeExclude`, inspect the effective
  governing configuration and its Git history. Remove it when its policy is inactive or its
  temporary condition has expired.
- Validate with a frozen/locked install, the resolved dependency tree, a fresh audit, and the
  smallest relevant build/test set. The final diff should contain only the intended resolution and
  still-necessary policy.

## Change discipline

- Keep `cargo fmt` clean and `cargo clippy --all-targets -- -D warnings` passing.
- Update `HANDOFF.md` in the same patch whenever its current state, architecture, defaults, trust
  boundaries, known limitations, versions, or working agreements become stale. Every release
  updates its date, released version, and explicit schema/analyzer version facts. Keep history in
  `CHANGELOG.md`; prune superseded detail from the handoff.
- Keep `ROADMAP.md` about unresolved decisions and evidence-gated future work. Summarize newly
  delivered work in at most one or two sentences, link to `CHANGELOG.md`, then remove that summary
  once the behavior is ordinary project context.
- Update `CHANGELOG.md` for every user-visible addition, behavior change, fix, removal,
  compatibility change, or meaningful performance improvement. Keep `[Unreleased]` first and
  prepend new bullets within its subsection. Pure refactors and test-only changes need no entry
  unless they materially affect supported behavior.
- Comment only where intent is not obvious. Prefer surgical changes and do not reformat or
  refactor unrelated code.
- Because Clap uses `args_conflicts_with_subcommands`, global flags follow subcommands, for example
  `reposcout tokens --encoding cl100k_base src/`.

## Validation before a commit

Run only checks relevant to the changed surface, in this order, and never concurrently. A complete
cross-stack change uses the full list:

1. Confirm `CHANGELOG.md` covers notable user-visible or performance changes and keeps newest
   entries first.
2. `cargo fmt --check`
3. `cargo clippy --all-targets -- -D warnings`
4. `cargo test`
5. `pnpm lint:frontend`
6. `pnpm build:web`
7. `pnpm test:web`
8. `pnpm build:landing`
9. `cargo build --release`
10. Sanity-run `reposcoutdev -f json .` and inspect the result.

Documentation-only changes do not require unrelated Rust/frontend builds. Dependency, release,
security, or cross-cutting changes require validation proportional to their actual risk even if
their textual diff is small.

Before pushing, verify every agent-created commit locally with `git log --show-signature` and every
third-party or bot commit through GitHub. Never use `--no-gpg-sign`, disable signing, or integrate
an unverifiable commit. GitHub's server-side rebase paths create unsigned replacement commits, so
rebase locally or fast-forward an up-to-date verified PR head. If signing is unavailable, stop and
ask the user.

## Testing notes

- Unit tests live beside their modules under `#[cfg(test)]`.
- Integration tests in `tests/cli.rs` run the compiled binary against
  `tests/fixtures/sample/`. Assertions target stable behavior such as tokens, lines, languages,
  markers, output formats, and `--fail-on`. The shared helper points `REPOSCOUT_GLOBAL_CONFIG` to
  `tests/fixtures/test-global.toml`, isolating developer settings and capping each CLI child at two
  workers; precedence tests explicitly override it. Repository Cargo configuration serializes the
  test harness, and process-I/O tests use bounded synthetic trees.
- `tests/dup_languages.rs` consumes `tests/fixtures/dup_languages.toml` and requires actionable
  exact and Type-2 findings for every canonical `lang::detect` format through the frozen detector
  APIs and CLI JSON contract. Keep its explicit 31-format set synchronized with language support.
- The sample fixture intentionally contains a duplicated block and TODO/FIXME/HACK markers. Keep
  them when editing fixtures or update the tests with the fixture.
