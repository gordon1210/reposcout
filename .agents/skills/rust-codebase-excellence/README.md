# Rust Codebase Excellence Skill

A production-oriented Agent Skill for designing, implementing, reviewing, refactoring, testing, securing, optimizing, and operating small-to-medium Rust codebases.

The skill is intentionally non-dogmatic. It preserves repository contracts and favors the smallest sound change over speculative architecture, blanket lint policies, or performative use of advanced Rust features.

## Contents

```text
rust-codebase-excellence/
├── SKILL.md
├── README.md
├── EVALUATION.md
├── LICENSE
└── references/
    ├── architecture-cargo-api.md
    ├── async-concurrency.md
    ├── documentation-observability-operations.md
    ├── idioms-types-errors.md
    ├── performance-resource-use.md
    ├── review-playbooks.md
    ├── security-dependencies-unsafe.md
    ├── sources.md
    ├── testing-verification.md
    └── tooling-baseline.md
```

`SKILL.md` contains the operating workflow and core rules. It routes the agent to focused references only when the task requires them, keeping normal context use small while retaining broad coverage for audits.

## Installation

Place the `rust-codebase-excellence` directory in the Agent Skills directory used by the host application. The directory name must remain identical to the `name` field in `SKILL.md`.

Examples of common locations vary by host; follow the host's current Agent Skills documentation rather than assuming a universal path.

## Intended use

Use the skill for:

- Focused Rust implementation and debugging.
- Patch and pull-request review.
- Repository-wide code-quality or security audits.
- Workspace, feature, MSRV, and public-API design.
- Async, concurrency, shutdown, and backpressure review.
- Unsafe and FFI review.
- Test and CI design.
- Performance and operational readiness work.

It is optimized for small-to-medium repositories but scales to a bounded package or subsystem inside a larger workspace.

## Design constraints

The skill deliberately avoids several common failure modes:

- No blanket ban on `unwrap` or `expect`; use depends on provable invariants and failure boundaries.
- No fixed byte threshold for passing values or deciding allocation.
- No blind `clippy::pedantic`, `clippy::restriction`, or `--all-features` policy.
- No automatic crate splitting, trait introduction, typestate, async conversion, or `Arc<Mutex<_>>`.
- No claim that running Cargo in unknown code is safe.
- No optimization without a defined contract and evidence.
- No review finding without a concrete trigger and impact.

## Validation

From the repository root, validate the package with the Agent Skills reference
validator when available:

```bash
skills-ref validate ./skills/rust-codebase-excellence
```

Also run the scenario checks in `EVALUATION.md` when materially changing the skill.

## Versioning

The skill uses semantic versioning in `SKILL.md` metadata:

- Patch: wording, source refresh, or clarification without changing expected agent decisions.
- Minor: additional guidance or a new reference that expands supported tasks compatibly.
- Major: changed rule hierarchy, required workflow, or behavior that can materially alter generated code or review outcomes.

## License

MIT. See `LICENSE`.
