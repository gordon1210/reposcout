# Documentation, Observability, and Operations

Read this reference for public APIs, command-line tools, services, background workers, configuration, telemetry, deployment behavior, persistence, and operational readiness.

Documentation and telemetry are part of the product contract. They must describe and expose actual behavior without leaking sensitive data or coupling callers to unstable internals.

## 1. Documentation hierarchy

Keep each fact at the narrowest useful level:

- Crate-level docs: purpose, architecture, core invariants, primary entry points, feature flags, and minimal examples.
- Module docs: responsibility, boundaries, and non-obvious invariants.
- Item docs: caller-visible semantics, units, errors, panics, safety, cancellation, blocking, allocation, and examples when useful.
- README: user-facing installation, quick start, compatibility, common workflows, and links to deeper docs.
- Architecture or decision records: cross-cutting decisions, alternatives, and consequences.
- Operations docs: configuration, deployment, health, recovery, migrations, alerts, and runbooks.

Do not duplicate mutable details across many documents. Link to one source of truth.

## 2. Public API documentation

Document what a caller cannot reliably infer from the signature:

- Preconditions and postconditions.
- Ownership and lifetime implications that affect use.
- Units, ranges, normalization, encoding, ordering, and determinism.
- Error categories and whether failures are retryable.
- Panic conditions when a public safe API can panic.
- Blocking behavior and thread or runtime requirements.
- Cancellation behavior for async operations.
- Side effects, persistence, idempotency, and atomicity.
- Feature or target availability.
- Complexity or resource behavior when material.
- Examples for the normal path and easy-to-misuse behavior.

Required conventional sections when applicable:

- `# Errors`
- `# Panics`
- `# Safety`
- `# Examples`

Do not promise stronger stability, ordering, timing, or performance than the implementation and compatibility policy support.

## 3. Examples and doctests

- Prefer small examples that compile and teach the intended API.
- Hide irrelevant setup in doctests rather than omitting required context.
- Use `no_run` only when execution requires external systems or side effects; keep compilation meaningful.
- Use `compile_fail` to document prohibited type-level use when stable diagnostics are not asserted verbatim.
- Do not place secrets, production endpoints, or environment-specific assumptions in examples.
- Keep examples compatible with declared MSRV and supported features.

## 4. Comments

Comments should explain:

- Why a non-obvious design exists.
- Which invariant makes code safe or correct.
- Why a workaround is necessary and when it can be removed.
- Why an apparently simpler alternative is invalid.
- The source of a protocol, algorithm, or compatibility rule.

Do not narrate syntax or preserve stale implementation history in comments. Use version control and decision records for history.

Every `unsafe` block requires a precise `SAFETY:` explanation. Public unsafe APIs require a `# Safety` contract.

## 5. Configuration design

Treat configuration as an external interface:

- Define precedence explicitly: defaults, files, environment, flags, remote configuration, or other sources.
- Validate once at startup or reload boundaries and convert to typed internal configuration.
- Distinguish absent, empty, zero, disabled, and inherited values.
- Use explicit units and parse human-friendly values into unambiguous internal types.
- Reject unknown keys when silent typos would be dangerous; allow forward-compatible extension only deliberately.
- Never log secret values. Redact or wrap secret-bearing types.
- Keep secure defaults and require explicit opt-in for dangerous modes.
- Document restart versus live-reload semantics and failure behavior during reload.
- Preserve backward compatibility or provide an explicit migration path.

Avoid a global configuration singleton when explicit dependency injection keeps tests and ownership clearer.

## 6. Command-line interfaces

A production CLI should provide predictable automation behavior:

- Stable exit codes with documented meaning when callers depend on them.
- Diagnostics on stderr and machine-consumable output on stdout.
- No decorative or progress output when a structured output mode is selected.
- Explicit non-interactive behavior; never block waiting for input unexpectedly in automation.
- Clear precedence among flags, environment, configuration files, and defaults.
- Safe handling of paths, Unicode, terminals, pipes, broken pipes, and redirected output.
- Atomic output or file replacement where partial writes would be harmful.
- Signal handling and cleanup appropriate to the operation.
- `--help` that describes semantics rather than merely restating option names.
- Version output that is reproducible and does not require network access.

Do not expose secrets in process arguments when environment variables, stdin, files with controlled permissions, or a secret provider are safer.

## 7. Service lifecycle

A service needs explicit lifecycle semantics:

1. Parse and validate configuration.
2. Initialize required resources.
3. Verify migrations or schema compatibility.
4. Start listeners and workers only when dependencies are ready enough for the declared readiness contract.
5. Serve while reporting health and useful telemetry.
6. Stop accepting new work during shutdown.
7. Drain, cancel, commit, or roll back in-flight work according to a deadline.
8. Flush bounded critical telemetry where appropriate.
9. Close resources and exit with a meaningful status.

Startup must fail loudly for required invariants. Optional dependencies should degrade only when the degraded behavior is explicit and observable.

## 8. Health endpoints

Separate concepts:

- Liveness: the process is running and not irrecoverably wedged.
- Readiness: the instance should receive new work.
- Startup: initialization is still in progress where the platform supports a separate probe.

Guidance:

- Do not make liveness depend on every remote dependency; transient dependency failure can create restart storms.
- Readiness may reflect dependencies required to serve correctly, but checks must be bounded and cheap.
- Avoid leaking internal topology, credentials, raw errors, or customer data.
- Define behavior during draining and migrations.
- Instrument probe failures without creating high-cardinality noise.

## 9. Logging and tracing

Use structured events rather than prose parsing when operations matter.

Each event should have:

- A stable event name or target.
- Severity appropriate to operator actionability.
- Relevant identifiers with bounded cardinality.
- Context propagated through request, job, or transaction spans.
- An error chain or classification when a failure occurs.
- No secrets or unnecessarily sensitive payloads.

Severity guidance:

- Error: an operation failed or invariant was violated and requires attention or affects service.
- Warning: unexpected degradation, recovery, or approaching limit that may require attention.
- Info: meaningful lifecycle or business-operational transition, not every request by default.
- Debug/trace: diagnostic detail disabled or sampled in normal production use.

Do not log the same error at every layer. Add context while propagating, then emit once at the boundary that owns reporting.

## 10. Spans and context propagation

- Create spans around meaningful operations, not every helper function.
- Propagate trace and correlation context across async tasks, queues, RPC, and background jobs where supported.
- Record outcome and latency at the owning boundary.
- Avoid holding span guards across async suspension when the tracing API requires instrumented futures instead.
- Keep field names stable enough for dashboards and alerts.
- Do not attach unbounded values or raw payloads to spans.

## 11. Metrics

Every metric needs a decision or diagnosis it supports.

Prefer:

- Counters for cumulative events.
- Gauges for current bounded state such as queue depth or active workers.
- Histograms for latency or size distributions with buckets appropriate to decisions.

Require:

- Stable names, units, and semantic definitions.
- Bounded label cardinality.
- A clear owner and expected action.
- Correct aggregation across processes and restarts.

Never use user IDs, request IDs, arbitrary URLs, raw errors, or unbounded strings as labels. Use logs or traces for high-cardinality diagnostics.

## 12. Telemetry privacy and security

- Classify telemetry fields and default to collecting less.
- Redact credentials, tokens, cookies, authorization headers, personal data, and payloads unless explicitly required and governed.
- Treat hashes as identifiers, not automatic anonymization.
- Apply retention, access control, tenant separation, and regional requirements.
- Prevent format-string or terminal-control injection when rendering untrusted values.
- Avoid telemetry paths that can block critical application work indefinitely.
- Define behavior when the collector is unavailable; telemetry failure should rarely take down the service.

## 13. Error reporting

Operational error reports should preserve:

- The top-level operation that failed.
- The causal chain.
- Stable classification for alerting or retry policy.
- Relevant safe context.
- A correlation identifier when detailed data lives elsewhere.

Avoid:

- Exposing debug representations directly to users or APIs.
- Turning every user error into an operator alert.
- Retrying permanent errors.
- Hiding repeated failures behind silent fallback.

## 14. Retries and resilience

- Retry only failures classified as transient and only when the operation is safe to repeat or has an idempotency mechanism.
- Use bounded attempts, deadlines, backoff, jitter, and a concurrency budget.
- Respect upstream retry hints where trustworthy.
- Prevent retry multiplication across layers.
- Expose retry exhaustion and degraded fallback through telemetry.
- Circuit breaking, hedging, and fallback add state and failure modes; introduce them only for a measured need with explicit semantics.

## 15. Persistence and migrations

Treat persisted representations as long-lived contracts:

- Define compatibility between application and schema versions.
- Make migrations deterministic, reviewable, observable, and recoverable.
- Use transactions where supported and where migration duration permits.
- Plan for large tables, locks, replication, backfills, and mixed-version deployments.
- Separate schema change from destructive cleanup when rolling deployment requires both representations temporarily.
- Back up and test restoration for destructive or irreversible operations.
- Never assume serialization format changes are harmless because Rust types still compile.

## 16. Background jobs and queues

- Define ownership, deduplication, idempotency, retry, ordering, visibility timeout, poison-message, and dead-letter behavior.
- Persist or acknowledge work at the correct point for delivery semantics.
- Bound concurrency and payload size.
- Make cancellation and shutdown behavior explicit.
- Emit queue age, depth, processing duration, outcomes, and retry metrics with bounded labels.
- Avoid exactly-once claims unless the complete end-to-end system proves them; most systems provide at-least-once or at-most-once components.

## 17. Time and clocks

- Use monotonic time for elapsed durations and deadlines.
- Use wall-clock time for human or external timestamps.
- Store timestamps in an unambiguous standard representation and convert for presentation.
- Define precision, timezone, leap-second assumptions, and clock-skew tolerance where material.
- Inject or abstract clocks when deterministic tests or time-dependent policy requires it.
- Do not derive security-sensitive ordering solely from unsynchronized wall clocks.

## 18. Shutdown and signals

- Centralize shutdown initiation and make it idempotent.
- Stop intake before draining dependent workers.
- Give shutdown a deadline and define forced termination behavior.
- Ensure spawned tasks can observe cancellation and are joined or deliberately aborted.
- Avoid waiting forever on a stuck dependency.
- Preserve data integrity over cosmetic cleanup.
- Test shutdown while idle, under load, during dependency failure, and during partially completed work when the risk justifies it.

## 19. Operational documentation

For deployed software, document:

- Required and optional configuration.
- Ports, files, permissions, external dependencies, and resource expectations.
- Startup, readiness, shutdown, and upgrade behavior.
- Data locations, backup, restore, and migration procedures.
- Common alerts and diagnosis steps.
- Safe rollback conditions and incompatibilities.
- Feature flags or emergency controls and their risks.
- Known failure modes and escalation criteria.

A runbook should contain executable checks and decision points, not vague advice.

## 20. Operational review checklist

Before release, verify:

- User-facing and operator-facing behavior is documented.
- Configuration is validated and secrets are protected.
- Logs, traces, and metrics are useful, bounded, and privacy-safe.
- Startup, readiness, liveness, draining, and shutdown semantics are coherent.
- Retries, timeouts, and backpressure are bounded.
- Persistent and wire-format compatibility is understood.
- Failure paths are observable without duplicate noise.
- Automation receives stable exit status and structured output where promised.
- Runbooks cover the failures that require human action.
