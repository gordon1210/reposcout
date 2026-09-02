# AGENTS.md

Guidance for coding agents and humans working in **reposcout**, a Rust CLI that gives agents and
humans a fast, consolidated view of repository size, health, duplication, structure, and change
impact. `README.md` is the user-facing contract; this file governs work in the repository.

## Mandatory reference routing

This file is intentionally compact. The focused documents under `docs/agents/` are **normative
extensions**, not optional background. Before editing or validating, read every reference matching
the affected area completely. A task spanning areas requires multiple references; uncertainty is a
reason to read the reference, not to guess. These are ordinary Markdown files, so do not assume
they were loaded automatically: this table is the required routing step. This root file always
applies.

| Required reference | Read it before… |
|---|---|
| [`repository-map.md`](docs/agents/repository-map.md) | selecting implementation files, moving responsibilities, or changing frontend structure |
| [`architecture-contracts.md`](docs/agents/architecture-contracts.md) | changing models, scanner flow, cacheable facts, discovery/config policy, outputs, or duplication interfaces |
| [`metrics.md`](docs/agents/metrics.md) | changing line, marker, complexity, duplication, risk, test-presence, assessment, or diagnostic semantics |
| [`reports-and-modes.md`](docs/agents/reports-and-modes.md) | changing report shapes/renderers, summary, change-summary, review, baseline, work-scope, or query behavior |
| [`graph-context-daemon.md`](docs/agents/graph-context-daemon.md) | changing graph resolution, impact, context planning, daemon graph generation, or graph UX |
| [`validation.md`](docs/agents/validation.md) | changing code or dependencies; running builds/tests; or interacting with processes, toolchains, fixtures, and releases |

When agent rules change, update the root router and every affected reference together. Do not let a
rule exist only in `HANDOFF.md`, a skill, or chat history.

## Non-negotiable working rules

### Keep `reposcoutdev` current

`~/.local/bin/reposcoutdev` is a symlink to this worktree's `target/release/reposcout`; the public
`reposcout` command is reserved for an installed release. After **every code change**, run:

```sh
cargo build --release
```

A debug build does not refresh the development command. See
[`validation.md`](docs/agents/validation.md) for symlink recovery and the full validation order.

### Respect process ownership

Never start, stop, restart, signal, kill, or otherwise control a daemon, frontend server, Vite,
watcher, browser, or other long-running process without explicit authorization for that interaction
in the current task. Treat every pre-existing or unknown process as user-owned. If authorized to
start one, record its exact session or PID and clean up only that process; never use broad name- or
port-based termination. Routine validation uses one-shot commands.

### Bound resource use

- Run at most one build, test suite, benchmark, or RepoScout scan at a time; never parallelize
  resource-intensive validation through shell jobs, tool calls, or subagents.
- Keep integration targets synthetic and bounded. Do not scan `CARGO_MANIFEST_DIR` for test data or
  run an unbounded scan/benchmark on a large external repository without explicit authorization.
- Do not override the serialized test harness, `RUST_TEST_THREADS`, the shared two-worker test
  configuration, or the shared CLI command helper during routine validation.
- Record and monitor authorized resource-intensive sessions. If one unexpectedly exceeds 180
  seconds, 1 GiB RSS, or one RepoScout child, stop only that recorded agent-owned session and report
  it. Fall back to the smallest relevant checks and disclose omissions.

### Keep changes narrow

Preserve unrelated user work. Make the smallest complete change, do not opportunistically refactor
adjacent code, and comment only where intent is not evident. Global flags follow subcommands because
Clap uses `args_conflicts_with_subcommands`, for example:

```sh
reposcout tokens --encoding cl100k_base src/
```

### Sign every commit

Every commit an agent creates or integrates must be cryptographically signed and verified before
push to `main`. Never use `--no-gpg-sign` or disable signing; if signing is unavailable, stop and
ask the user.

## Core architecture invariants

- `src/model.rs` is the stable serializable API shared by analyzers and reporters. JSON changes are
  additive with Serde defaults; bump `SCHEMA_VERSION` only for breaking changes and update contract
  tests with it.
- Bump `ANALYZER_VERSION` in `src/cache.rs` whenever cached `FileReport` facts or semantics change.
  `AnalysisProfile` must include every runtime setting that changes those facts. Summary-only and
  top-level projections do not require a bump.
- `src/scan.rs` owns orchestration. Reuse its analyzers and cache rather than adding parallel scan,
  explain, query, parser, index, or cache pipelines.
- Public analyzer signatures documented in
  [`architecture-contracts.md`](docs/agents/architecture-contracts.md) stay stable. In particular,
  `dup::exact::detect` and `dup::fuzzy::detect` are frozen adapters. Cross-cutting duplication
  policy belongs in `src/dup/mod.rs` orchestration.
- Complete inventory and actionable health are separate contracts. Every recognized format keeps
  inventory/token/line/navigation facts; health scope controls complexity, markers, duplication,
  risk, test-presence, and cleanup evidence. Path exclusions apply last. Never let health policy
  silently narrow inventory.
- Minified and recognized bundled/chunk output remains visible to inventory/navigation but is
  excluded from duplication by default. Only explicit `--dup-include-artifacts` or
  `duplication_include_artifacts = true` opts it back in.
- Output paths are exact canonical scan exclusions, never globs. Writes must remain atomic and
  symlink-safe. Cache data belongs in the OS cache directory and must never modify a scanned repo.
- Configuration precedence is CLI, nearest project config, global config, then defaults. Preserve
  independent nested merges, array replacement semantics, explicit CLI list extension, and the
  `--no-project-config` trust boundary.
- CLI graph facts stay lazy until graph/context/impact/explain work. Daemon refreshes retain
  revision-scoped facts and configs but defer topology to `/api/graph`, preventing old revisions
  from rereading live files. Other scans stay graph-free without a graph consumer; report formats
  remain pure projections of shared `ScanReport` facts.
- Stable CLI JSON/NDJSON and shared query contracts are the automation surface. Do not add an MCP
  dependency or a second task-query implementation.

## Repository ownership and maintenance

### Documentation

- `HANDOFF.md` is the maintained current-state contract. Update it in the same patch when
  architecture, defaults, trust boundaries, known limitations, versions, or working agreements
  change. Every release updates its date, released version, and schema/analyzer facts. Keep history
  in `CHANGELOG.md` and prune superseded handoff detail.
- `ROADMAP.md` contains unresolved decisions and evidence-gated future work. Delivered work gets at
  most one or two temporary sentences linked to `CHANGELOG.md`, then disappears once integrated.
- Update `CHANGELOG.md` for every user-visible behavior, compatibility, security, removal, or
  meaningful performance change. Keep `[Unreleased]` first and prepend entries within a section.
- Keep this root file comfortably below the Codex project-instruction ceiling; target at most
  approximately 9.6 KiB. Put detailed agent guidance in the routed references without weakening
  the root safety rules or core invariants.

### Skills

`skills/reposcout` is the canonical bundled RepoScout skill;
`.agents/skills/reposcout` is its deterministic repository mirror. Edit only the canonical copy,
then run `./scripts/reposcout-skill.sh sync` and `check`. All other `.agents/skills/` entries are
installed through skills.sh and `skills-lock.json`; never edit those installed copies manually.
RepoScout's own mirror must not appear in `skills-lock.json` as externally managed.

### Dependencies

Trace the complete dependency path before changing manifests or lockfiles. For transitive findings,
first use the package manager's targeted lockfile/security route (for pnpm,
`pnpm audit --fix=update`) and inspect the diff. A no-op direct update does not justify an override.

Overrides, resolutions, transitive pins, ignores, and release-age exclusions are last resorts. Use
one only after proving normal constraints cannot select a patched version; keep it narrow and
document why it exists and exactly when to remove it. Inspect governing configuration and history
before retaining an existing exception. Validate the final state with a frozen install, resolved
tree, fresh audit, and the smallest relevant build/tests.

## Validation baseline

Rust uses edition 2024 through `rustup`; frontend packages use the root pnpm workspace and the
`packageManager` pin in `package.json`. Validation is proportional to the changed surface and runs
sequentially. The standard code path is:

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
cargo build --release
```

Frontend changes additionally use the relevant `pnpm lint:frontend`, `build:web`, `test:web`, and
`build:landing` commands. Documentation-only changes do not require unrelated builds. Before
finishing, inspect the final diff, run `git diff --check`, validate affected links/contracts, and
state any skipped checks. Full commands, fixture invariants, dependency checks, and release-build
requirements live in [`validation.md`](docs/agents/validation.md).
