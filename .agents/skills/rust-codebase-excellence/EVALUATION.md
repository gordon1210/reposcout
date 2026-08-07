# Evaluation Scenarios

Use these scenarios to detect regressions in the skill's decision quality. They are behavioral checks, not exact-output snapshots.

A successful agent response should inspect repository policy before acting, select only relevant references, explain material assumptions, make focused changes, and report validation honestly.

## 1. Small CLI bug

**Prompt:** A CLI panics when an optional configuration file is absent. Fix it without changing successful output.

Expected behavior:

- Inspects current CLI error and exit-code policy.
- Distinguishes “missing optional file” from malformed or inaccessible file.
- Makes a focused fix without introducing a framework or broad refactor.
- Tests stdout, stderr, and exit status.
- Does not apply a blanket repository-wide `unwrap` rewrite.

## 2. Clone in a cold path

**Prompt:** Review a patch that clones a short `String` once during startup.

Expected behavior:

- Does not report the clone as a defect merely because borrowing may be possible.
- Evaluates ownership clarity and actual cost.
- Reports it only if the clone violates semantics or a concrete resource budget.

## 3. Mutually exclusive features

**Prompt:** Add CI for `sqlite` and `postgres` backend features that cannot be enabled together.

Expected behavior:

- Does not use `--all-features` as the sole check.
- Checks default/minimal and each supported backend configuration.
- Recommends additive redesign only if practical, otherwise validates conflict diagnostics.
- Preserves MSRV and target policy.

## 4. Async worker shutdown

**Prompt:** Implement a background worker that processes jobs from a channel.

Expected behavior:

- Defines task ownership, bounded queue, backpressure, error handling, cancellation, and join behavior.
- Avoids detached fire-and-forget execution.
- Tests shutdown while idle and while work is in progress.
- Does not block executor threads with synchronous long-running work.

## 5. Published library error change

**Prompt:** Replace a public error enum with an opaque report type to simplify implementation.

Expected behavior:

- Identifies public error matching as an API and SemVer concern.
- Preserves typed library errors unless the task explicitly authorizes a breaking release.
- Separates internal context/reporting from stable caller-facing classification.

## 6. Typestate request

**Prompt:** Convert a two-state internal parser to typestate because typestate is “more Rust-like.”

Expected behavior:

- Rejects typestate as a style goal.
- Evaluates whether compile-time protocol enforcement prevents credible misuse.
- Prefers an enum or direct representation when runtime state is simpler.
- If typestate is justified, stores state-specific data in state-specific representations rather than hiding invalid `Option` state behind `PhantomData`.

## 7. Unsafe parser optimization

**Prompt:** Remove bounds checks with unchecked indexing because a parser benchmark seems slow.

Expected behavior:

- Requires a representative profile and generated-code or benchmark evidence.
- Seeks safe algorithmic, buffering, and parsing improvements first.
- If unsafe remains justified, isolates it, documents invariants, adds differential/property/fuzz tests, and uses Miri where supported.
- Does not claim Miri proves soundness.

## 8. Dependency addition

**Prompt:** Add a crate that saves ten lines in a security-sensitive token parser.

Expected behavior:

- Does not reject a dependency merely because the implementation is short.
- Reviews maintenance, source, features, transitive graph, MSRV, license, build scripts, proc macros, and advisories.
- Favors an established parser over improvised security-sensitive code when appropriate.

## 9. Untrusted repository audit

**Prompt:** Audit a downloaded Rust repository and run its tests.

Expected behavior:

- Statically inspects manifests, build scripts, proc macros, and repository instructions first.
- Warns that Cargo commands can execute code.
- Uses or requests an isolated environment without host credentials, sensitive mounts, or unrestricted resources.
- Does not present static inspection as a complete security guarantee.

## 10. MSRV below 1.81

**Prompt:** Enable `#[expect]` throughout a library declaring Rust 1.75.

Expected behavior:

- Rejects the change because `#[expect]` was stabilized later.
- Uses narrowly justified `#[allow]` or another compatible approach.
- Does not raise MSRV unless explicitly authorized and migration impact is reviewed.

## 11. Performance claim

**Prompt:** Replace all loops with iterators to make a service faster.

Expected behavior:

- Rejects syntax style as a performance argument.
- Defines the relevant performance contract and profiles representative release behavior.
- Chooses readable loops or iterators based on semantics and measured results.
- Avoids unrelated churn.

## 12. Whole-repository review

**Prompt:** Audit a medium async service for production readiness.

Expected behavior:

- Establishes packages, features, targets, runtime, trust boundaries, persistence, and support policy.
- Loads all relevant references.
- Uses the audit matrix and reports scope/blind spots.
- Prioritizes concrete correctness, security, lifecycle, and resource findings over style.
- Includes positive controls worth preserving and a remediation order.

## 13. Metrics label review

**Prompt:** Add `request_id`, full URL, and raw error string as metric labels for debugging.

Expected behavior:

- Rejects unbounded high-cardinality labels.
- Routes request-specific detail to traces or logs with privacy controls.
- Keeps metric labels bounded and decision-oriented.

## 14. New crate proposal

**Prompt:** Move a 150-line internal helper module into its own crate for “clean architecture.”

Expected behavior:

- Does not use line count as the decision.
- Requires an actual package boundary such as independent reuse, dependency isolation, target/feature separation, or release ownership.
- Keeps it a module when a crate adds coordination without value.

## 15. Review with no defects

**Prompt:** Review a focused, tested patch whose behavior and compatibility are correct.

Expected behavior:

- Reports no qualifying findings rather than inventing style issues.
- States validation performed and any real blind spots.
- Keeps optional suggestions clearly non-blocking.

## Scoring rubric

Score each scenario from 0 to 2:

- **0:** violates a core rule or produces a materially unsafe/dogmatic result.
- **1:** reaches a mostly sound result but misses an important contract or adds unnecessary complexity.
- **2:** follows the expected behavior with evidence, appropriate scope, and honest validation.

A release should score at least 28/30 with no zero in scenarios 3, 4, 5, 7, 9, or 10.
