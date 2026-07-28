<p align="center">
  <img src="apps/web/src/assets/reposcout.png" alt="RepoScout" width="640">
</p>

# reposcout

> Fast repository scout — tokens, complexity, duplication & health metrics for a repo or any path inside it.

`reposcout` is a Rust CLI that scans a Git repository (or a subdirectory / single file) and prints
a **consolidated status** of the code in seconds. It is built for two audiences:

- **Agents** — stable, machine-readable JSON to quickly understand a codebase.
- **Humans** — a compact, colored terminal summary (or Markdown for PRs/issues).

<p>
  <a href="https://github.com/gordon1210/reposcout/actions/workflows/release.yml"><img alt="Release" src="https://github.com/gordon1210/reposcout/actions/workflows/release.yml/badge.svg"></a>
  <a href="https://github.com/gordon1210/reposcout/releases/latest"><img alt="Latest release" src="https://img.shields.io/github/v/release/gordon1210/reposcout?display_name=tag&sort=semver"></a>
  <a href="LICENSE"><img alt="License: MIT" src="https://img.shields.io/badge/License-MIT-yellow.svg"></a>
  <img alt="Platforms: Apple Silicon macOS and x86-64 Linux" src="https://img.shields.io/badge/platform-Apple%20Silicon%20macOS%20%7C%20x86--64%20Linux-blue">
</p>

## See the repository before reading it

RepoScout combines signals that usually require several tools:

| Question | RepoScout answers with |
|---|---|
| **What is here?** | Languages, files, SLOC, tokens, symbols, imports, and repository size |
| **Where is the risk?** | Per-callable complexity, maintainability, churn, test presence, and composite risk |
| **What can be skipped?** | Generated, minified, vendored, and oversized-file hints |
| **Is cleanup worthwhile?** | Exact and Type-2 duplication, markers, hotspots, and an evidence-qualified assessment |
| **What should be read next?** | A deterministic context plan under hard token and file budgets |
| **What could this change affect?** | Diff-scoped review, dependency/type graphs, and direct/transitive impact |

Everything runs locally. RepoScout does not upload source, call a model, or write analysis state
into the repository it scans.

## Highlights

- **Source-first health:** complete repository inventory without letting documentation, generated
  data, or styles dilute code-health metrics.
- **Actionable complexity:** cyclomatic, cognitive, nesting, Halstead, and Maintainability Index
  with precise callable-level findings.
- **Structured duplication:** format-aware exact and consistent-rename Type-2 clones with precise
  locations and union-based coverage.
- **Agent-ready queries:** compact summary mode, capability discovery, declaration lookup,
  structured errors, and guardrailed execution profiles.
- **Explainable context:** bounded reading plans that rank focus paths, changes, tests,
  dependencies, dependents, risk, and repository instructions.
- **Change intelligence:** Git diff scopes, changed-line/deep review, finding baselines, impact
  analysis, DOT/Mermaid exports, and SARIF.
- **Live local dashboard:** an optional daemon and React interface for repository health,
  findings, files, and mixed-language architecture.

First-class AST languages are **Rust, Python, JavaScript, TypeScript/TSX, Go, and PHP**.
RepoScout recognizes 31 source and content formats for complete inventory and structured
duplication.

## Install

Install the latest stable GitHub release on Apple Silicon macOS or x86-64 Linux:

```sh
curl --proto '=https' --tlsv1.2 -LsSf https://getreposcout.vercel.app/install.sh | sh
```

Update an installer-managed copy:

```sh
reposcout update
```

The installer chooses the correct prebuilt archive, requires successful checksum verification, and
installs the binary under Cargo's binary directory. Built-in updates download and verify the
immutable release archive directly; they never execute a downloaded installer script. See
[Getting started](docs/getting-started.md) for source builds, release attestations, the agent
skill, and default behavior.

## Install the agent skill

Install the companion RepoScout skill for supported coding agents through
[skills.sh](https://www.skills.sh/gordon1210/reposcout/reposcout):

```sh
npx skills add gordon1210/reposcout --skill reposcout
```

The `reposcout` binary must already be available on `PATH`.

## Quick start

```sh
# Human summary of the current repository
reposcout

# Compact agent-oriented JSON
reposcout -f json --summary --profile agent .

# Find one declaration across supported languages
reposcout locate HttpClient . --exact -f json

# Build a reading plan under hard limits
reposcout -f json --summary --context-budget 24000 --context-max-files 15 .

# Review the current working-tree change
reposcout --working --review .
```

`PATH` may be a repository root, subdirectory, or single file. The surrounding Git root is
detected automatically.

## Common workflows

### Scout an untrusted checkout

```sh
reposcout -f json --summary --profile safe .
```

The `safe` profile ignores repository configuration and applies conservative file, byte, time,
worker, history, context, and duplication limits. Partial scans identify every resource bound that
was reached instead of presenting missing analysis as a clean result.

### Understand a change

```sh
reposcout -f json --summary --since main --context --impact .
reposcout -f sarif --since main --review=deep --fail-on-review .
```

Metrics remain change-scoped while context and impact consult unchanged tests and topology.

### Inspect architecture

```sh
reposcout --graph .
reposcout --graph-focus src/service.ts --graph-direction dependents \
  --graph-depth 2 -f mermaid -o service-radius.mmd .
```

The graph covers imports and explicit type relationships across every first-class language while
keeping resolver provenance and ambiguity visible.

### Run the live dashboard

```sh
reposcout daemon .
pnpm dev:web
```

The daemon binds to loopback by default, rejects browser-origin and DNS-rebinding requests that do
not match its local trust boundary, and refuses non-loopback listeners unless the risk is explicitly
accepted. See [Daemon and web dashboard](docs/daemon-and-web.md) for setup and the local API.

## Documentation

| Guide | Contents |
|---|---|
| [Getting started](docs/getting-started.md) | Installation, first scan, formats, languages, and defaults |
| [CLI reference](docs/cli-reference.md) | Commands, grouped options, examples, gates, and debug logs |
| [Agent workflows](docs/agent-workflows.md) | Summary scouting, context, locate, explain, impact, and review |
| [Metrics and interpretation](docs/metrics.md) | Complexity, duplication, risk, tests, assessment, and limits |
| [Configuration and caching](docs/configuration.md) | Precedence, profiles, project policy, ignores, and cache behavior |
| [Reports and machine formats](docs/report-formats.md) | JSON, NDJSON, SARIF, Markdown, DOT, Mermaid, and exit codes |
| [Daemon and web dashboard](docs/daemon-and-web.md) | Watch mode, HTTP API, dashboard routes, and graph explorer |
| [Development](docs/development.md) | Toolchain, tests, architecture, contracts, and release builds |

Start from the [documentation index](docs/README.md) or ask the installed binary what it supports:

```sh
reposcout capabilities -f json
```

## Project

- [Contributing](CONTRIBUTING.md)
- [Security policy](SECURITY.md)
- [Changelog](CHANGELOG.md)
- [Roadmap](ROADMAP.md)
- [GitHub Releases](https://github.com/gordon1210/reposcout/releases)
- [Third-party notices](THIRD_PARTY_NOTICES.md)

## License

RepoScout is available under the [MIT License](LICENSE).
