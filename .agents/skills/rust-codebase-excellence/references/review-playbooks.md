# Review and Audit Playbooks

Read this reference when reviewing a patch, auditing a repository, assessing a migration, or reporting findings. The goal is evidence-based risk reduction, not stylistic churn.

## 1. Review principles

- Review the requested scope first; do not bury the answer under unrelated repository issues.
- Understand the repository contract before judging a design.
- Prefer concrete failure modes and maintainability costs over generic best-practice claims.
- Distinguish correctness defects from optional improvements.
- Do not demand a rewrite when a focused fix removes the risk.
- Do not assume code is unused, unreachable, private, or unsupported without evidence.
- Treat generated, vendored, fixture, migration, and compatibility code according to its role.
- Confirm language, library, Cargo, target, and runtime behavior from primary documentation when uncertain.

## 2. Review modes

### Patch review

Focus on changed behavior and risks introduced or exposed by the diff.

Inspect:

- The task or issue and acceptance criteria.
- The complete diff, not only isolated hunks.
- Callers, tests, types, and contracts adjacent to changed code.
- Error, cancellation, cleanup, retry, and compatibility paths.
- Generated changes such as `Cargo.lock`, schemas, snapshots, or API output.

Do not turn patch review into a general repository audit unless a pre-existing issue directly makes the patch unsafe.

### Repository audit

Establish scope explicitly: packages, targets, feature sets, platforms, trust boundaries, and time budget.

Audit in passes:

1. Architecture, Cargo metadata, and compatibility policy.
2. Public API and data or protocol boundaries.
3. Correctness and error handling.
4. Async, concurrency, lifecycle, and resource bounds.
5. Security, dependencies, build execution, and `unsafe`.
6. Tests, CI, feature matrix, MSRV, and target coverage.
7. Performance and operational behavior where relevant.
8. Documentation and maintenance risks.

A repository audit cannot prove absence of defects. State coverage and blind spots precisely.

### Design review

Review before implementation:

- Requirements and non-requirements.
- State, ownership, trust, and failure boundaries.
- Public and persisted contracts.
- Concurrency, cancellation, idempotency, and recovery.
- Alternatives and why complexity is justified.
- Validation strategy and rollout or migration plan.

Reject abstractions that solve only hypothetical future requirements.

### Migration review

For edition, MSRV, runtime, framework, database, serialization, protocol, or dependency migrations:

- Separate mechanical changes from semantic changes.
- Define old/new compatibility windows.
- Identify irreversible steps and rollback limits.
- Validate behavior on both sides of mixed-version deployment where applicable.
- Review generated lockfile and transitive dependency changes.
- Require targeted regression tests for changed semantics.

## 3. Severity model

Use the repository's established severity model when one exists. Otherwise use:

### Critical

A credible path to catastrophic impact such as remote code execution, key or broad sensitive-data compromise, systemic data loss or corruption, unsafe-code unsoundness exploitable through safe callers, or complete security-boundary bypass.

### High

A likely or material correctness, security, availability, or compatibility failure with broad impact, difficult recovery, or no practical mitigation. Examples include cross-tenant access, persistent corruption, deadlock in a core path, uncontrolled resource exhaustion from untrusted input, or a breaking public API change presented as compatible.

### Medium

A real defect with bounded impact, conditional trigger, recoverable failure, or meaningful maintenance risk. Examples include an incorrect edge case, task leak under a specific cancellation path, retry amplification, missing validation on a limited boundary, or unsupported feature combination that CI claims to support.

### Low

A concrete but small reliability, diagnosability, compatibility, or maintenance issue unlikely to cause serious immediate impact. It still needs a plausible failure mode or cost.

### Suggestion

A non-defect improvement, simplification, naming change, or future hardening idea. Do not present suggestions as required fixes.

Severity reflects impact and likelihood in the actual system, not how much code is involved.

## 4. Confidence

State confidence when evidence is incomplete:

- High: directly demonstrated by code, tests, specification, or reproducible behavior.
- Medium: strongly implied but depends on an unverified caller, deployment detail, or platform behavior.
- Low: plausible hypothesis requiring targeted confirmation; normally ask for evidence or omit it from blocking findings.

Do not inflate severity to compensate for low confidence.

## 5. Finding quality bar

A finding must contain:

1. **Title:** concrete failure, not a rule name.
2. **Location:** smallest useful file and line range.
3. **Trigger:** input, state, timing, feature, target, or call sequence required.
4. **Impact:** observable consequence.
5. **Evidence:** code path, contract, test, documentation, or reproduction.
6. **Remediation:** smallest sound direction, without prescribing an unnecessary rewrite.
7. **Severity and confidence.**

Example structure:

```markdown
### High — Cancellation can acknowledge an uncommitted job

`src/worker.rs:84-113`

After `acknowledge()` completes, the future can be cancelled while the database write is still pending. The queue will not redeliver the job, but the state change may never commit, causing permanent data loss during shutdown or timeout cancellation.

Move acknowledgement after the transaction commits, or make the operation idempotent and recoverable. Add a test that cancels at the boundary between both operations.

Confidence: high.
```

Do not report:

- A lint preference with no failure or maintenance cost.
- A hypothetical race without a concurrent path.
- An allocation without evidence it matters.
- A clone merely because a borrow might be possible.
- A public API break without confirming the item is public and covered by compatibility expectations.
- A panic that is provably behind a local invariant, unless the invariant can be violated.
- Missing tests as a standalone defect when no important unverified behavior is identified.

## 6. Patch review procedure

### Step 1: Restate the behavioral delta

Identify:

- What changes for callers or operators.
- What must remain compatible.
- New states, errors, tasks, dependencies, or persisted data.
- Which validation commands should establish correctness.

### Step 2: Trace affected paths

Trace at least:

- Normal success.
- Invalid or boundary input.
- Dependency failure.
- Partial progress and cleanup.
- Panic or invariant breach where relevant.
- Timeout and cancellation in async code.
- Concurrent access and shutdown.
- Feature-disabled and target-specific compilation when relevant.

### Step 3: Inspect contracts

Check:

- Public signatures and trait behavior.
- Error variants and matching expectations.
- Serialization, schema, protocol, and CLI output.
- MSRV, edition, features, target cfgs, and runtime assumptions.
- Security and tenant boundaries.
- Resource limits and operational telemetry.

### Step 4: Inspect tests

Determine whether tests:

- Fail against the old defect or absent behavior.
- Exercise the meaningful boundary rather than an implementation detail.
- Remain deterministic and parallel-safe.
- Cover negative and cleanup behavior proportionate to risk.
- Run in the CI feature and target matrix.

### Step 5: Validate selectively

Use repository commands first. Begin with focused checks, then expand according to risk. Never claim unexecuted validation.

### Step 6: Re-read the final diff

Look for accidental lockfile churn, generated artifacts, debugging code, broad lint suppressions, unrelated formatting, visibility expansion, new clones, and stale comments.

## 7. Repository audit matrix

Use only applicable rows and record evidence:

| Area | Questions |
|---|---|
| Architecture | Are package and module boundaries coherent? Are dependencies directional? |
| Cargo | Are MSRV, edition, features, targets, profiles, and lockfile policy explicit? |
| API | Are public contracts minimal, documented, and evolution-safe? |
| Types | Are units, validated values, ownership, and invalid states represented safely? |
| Errors | Are recoverable errors typed and contextual? Are panics justified? |
| Async | Are blocking, cancellation, task ownership, shutdown, and backpressure correct? |
| Concurrency | Are lock scope, ordering, atomics, and shared ownership justified? |
| Input | Are size, recursion, encoding, path, parser, and decompression limits enforced? |
| Security | Are authn/authz, tenant isolation, secrets, process and network boundaries sound? |
| Dependencies | Are features, build scripts, proc macros, licenses, advisories, and sources reviewed? |
| Unsafe | Are invariants documented, blocks minimal, and safe APIs sound? |
| Persistence | Are migrations, transactions, compatibility, backup, and recovery defined? |
| Tests | Are critical behavior and regressions covered deterministically? |
| CI | Does CI cover supported features, MSRV, targets, docs, lint, and security policy? |
| Performance | Are hot paths measured and resource growth bounded? |
| Operations | Are telemetry, health, retries, timeouts, and lifecycle coherent? |
| Documentation | Can users and maintainers operate and evolve the code safely? |

## 8. Common high-value Rust review checks

### Ownership and lifetimes

- Clones hiding unclear ownership or expensive duplication.
- References retained beyond the lifetime or synchronization model they imply.
- `Arc` used as a default instead of defining ownership.
- Reference cycles or permanently retained caches.

### Errors and panics

- Lost source errors or erased classifications callers need.
- `unwrap` or indexing reachable from untrusted or ordinary runtime input.
- Error messages leaking secrets.
- Partial mutation before an error without rollback or documented semantics.

### Async and concurrency

- Synchronous blocking on executor threads.
- Locks or guards held across `.await` unintentionally.
- Detached tasks whose errors, lifetime, or shutdown are unowned.
- Unbounded channels, fan-out, retries, or buffering.
- Cancellation between irreversible steps.
- Deadlock through inconsistent lock order or callback re-entry.

### Cargo and compatibility

- New syntax or APIs above the declared MSRV.
- `--all-features` assumed valid despite mutually exclusive modes.
- Default-feature changes that silently alter downstream behavior.
- Target-specific dependencies or cfg branches not checked.
- Public types or variants changed without SemVer analysis.
- Lockfile changes unrelated to the patch.

### Security and unsafe

- Build scripts or proc macros added without trust review.
- Paths joined without traversal or symlink policy.
- Command arguments crossing a shell unnecessarily.
- Missing input, allocation, decompression, or recursion limits.
- Authorization checked on one entry path but not another.
- Unsafe invariants relying on undocumented caller behavior.
- FFI ownership, panic, string, layout, or thread assumptions mismatched.

### Operations

- Retry loops without deadlines, jitter, idempotency, or bounds.
- High-cardinality metric labels or sensitive logging.
- Health checks that trigger restart storms.
- Shutdown that drops accepted work silently.
- Configuration that fails open or accepts misspelled dangerous settings.

## 9. False-positive controls

Before filing a finding, attempt to disprove it:

- Search for validation earlier in the call path.
- Check type invariants and constructors.
- Check feature and cfg gates.
- Check whether a wrapper owns cleanup or reporting.
- Check whether the operation is intentionally process-fatal.
- Check tests that define intended behavior.
- Check documented deployment assumptions.
- Confirm the relevant version of Rust, Cargo, runtime, dependency, or platform.

If the conclusion depends on missing deployment context, state the dependency instead of presenting certainty.

## 10. Review output format

For a code review:

1. Findings ordered by severity, then impact.
2. Open questions or assumptions only when they affect correctness.
3. Brief validation summary.
4. Optional non-blocking suggestions clearly separated.

If no qualifying findings exist, say so directly and list meaningful validation gaps. Do not invent a finding to make the review appear useful.

For an audit:

1. Executive assessment and scope.
2. Findings by severity.
3. Coverage matrix and methods.
4. Positive controls worth preserving.
5. Prioritized remediation sequence.
6. Residual risks and unverified areas.

## 11. Remediation prioritization

Prioritize by risk reduction per unit of change:

1. Contain active security, corruption, or availability risk.
2. Add a regression test or invariant check that reproduces the failure.
3. Apply the smallest correct fix.
4. Add broader hardening where the same root cause exists.
5. Refactor only when necessary to make correctness understandable and maintainable.

Do not combine large architectural cleanup with an urgent fix unless the existing shape prevents a safe focused change.

## 12. Completion gate

A review is complete when:

- The requested scope and supported matrix are clear.
- Changed and high-risk paths were traced.
- Findings meet the evidence bar and duplicates are consolidated.
- Severity reflects actual impact and likelihood.
- Suggested fixes preserve higher-priority contracts.
- Executed validation and remaining blind spots are stated accurately.
