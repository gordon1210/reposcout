# Getting started

← [Documentation index](README.md)

RepoScout scans a Git repository, a directory inside one, or a single file. It reports a
consolidated view of repository size, complexity, duplication, health, structure, and change risk.

## Install

Install the latest stable GitHub release on Apple Silicon macOS or x86-64 Linux:

```sh
curl --proto '=https' --tlsv1.2 -LsSf https://getreposcout.vercel.app/install.sh | sh
```

The installer selects the matching prebuilt archive and refuses to install it unless SHA-256
verification succeeds. It installs the binary under Cargo's binary directory and stores an
owner-writable install receipt for later updates.

Update an installer-managed copy later with:

```sh
reposcout update
```

`reposcout update` follows the latest stable, immutable GitHub Release. It downloads the exact
platform archive with its embedded TLS client, verifies the SHA-256 digest reported by GitHub, and
replaces the binary and receipt atomically with rollback on receipt failure. It never executes a
downloaded shell script and refuses source builds or executables whose version or location does
not match the installer's receipt.

Release archives and checksums are available on
[GitHub Releases](https://github.com/gordon1210/reposcout/releases).
Published artifacts also carry GitHub build-provenance attestations:

```sh
gh attestation verify reposcout-aarch64-apple-darwin.tar.xz \
  --repo gordon1210/reposcout
```

The short install URL is a convenience HTTPS redirect. To remove the landing host from the
bootstrap trust path, download the same immutable release installer directly from GitHub:

```sh
curl --proto '=https' --tlsv1.2 -LsSf \
  https://github.com/gordon1210/reposcout/releases/latest/download/reposcout-installer.sh | sh
```

## Build from source

Building requires a Rust toolchain and a C compiler for the vendored tree-sitter grammars and
libgit2:

```sh
cargo build --release
./target/release/reposcout --version
```

## Run the first scan

From any location inside a Git repository:

```sh
reposcout
```

Or name a repository, directory, or file explicitly:

```sh
reposcout path/to/repository
reposcout src/
reposcout src/service.ts
```

RepoScout detects the surrounding Git root so Git churn and repository-relative paths remain
useful even when the target is a subdirectory.

## Choose an output

Interactive terminals receive the compact table report. Redirected output defaults to JSON.

```sh
reposcout .                         # human table
reposcout -f json .                # complete machine report
reposcout -f json --summary .      # compact scouting report
reposcout -o report.md .           # format inferred from extension
reposcout -o report.sarif .        # SARIF 2.1.0
```

See [Reports and machine formats](report-formats.md) for the output contracts.

## Understand the defaults

RepoScout separates complete inventory from source-health analysis:

- Every recognized format contributes file, byte, token, and line totals.
- Marker and duplication analysis defaults to programming/build source.
- HTML, CSS/SCSS, JSON, YAML, TOML, Markdown, XML, and text enter health analysis only when
  explicitly included.
- Dependency lockfiles are skipped from repository scanning by default.
- `.gitignore` and hierarchical `.reposcoutignore` files are respected.
- Analysis and Git-history caches live in the operating system's cache directory, never inside
  the scanned repository.

First-class AST languages are **Rust, Python, JavaScript, TypeScript/TSX, Go, and PHP**.
Other recognized code formats still contribute inventory and heuristic signals where appropriate.

Opt a content format into health analysis:

```sh
reposcout --health-include css --health-include scss .
```

Or deliberately analyze every recognized format:

```sh
reposcout --health-scope all .
```

## Install the agent skill

The RepoScout skill teaches supported coding agents to use compact JSON, context planning,
symbol lookup, graph, impact, and review workflows:

```sh
npx skills add gordon1210/reposcout --skill reposcout
```

The `reposcout` binary must already be available on `PATH`.

## Where to go next

- Use [Agent workflows](agent-workflows.md) for bounded reading plans and change analysis.
- Use the [CLI reference](cli-reference.md) for every command and option.
- Use [Configuration and caching](configuration.md) to save team or personal defaults.
- Start the [daemon and web dashboard](daemon-and-web.md) for a live local view.
