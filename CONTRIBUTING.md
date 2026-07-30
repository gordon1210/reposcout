# Contributing to RepoScout

Thanks for helping improve RepoScout.

## Before you start

- Search existing issues before opening a new one.
- Open an issue before undertaking a large feature or architectural change.
- Keep pull requests focused on one problem and avoid unrelated refactors.
- Report vulnerabilities through the [security policy](SECURITY.md), not a public issue.

## Development setup

RepoScout requires Rust with the 2024 edition toolchain, a C compiler, and pnpm for the web
workspaces.

```sh
cargo build
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt --check
pnpm lint:frontend
pnpm build:web
pnpm test:web
pnpm build:landing
```

Use the smallest relevant validation while developing, then run the complete applicable checks
before submitting a pull request. See the [development guide](docs/development.md) for the
repository layout, stable contracts, and common commands.

## Pull requests

A useful pull request explains:

- the problem and intended behavior;
- the chosen approach and important tradeoffs;
- how the change was validated;
- any user-visible changes that belong in `CHANGELOG.md`.

Changes to serialized report fields must preserve the compatibility rules documented in the
[development guide](docs/development.md). Code changes should include focused tests for the
behavior they add or fix.

By contributing, you agree that your contribution is licensed under the
[MIT License](LICENSE).
