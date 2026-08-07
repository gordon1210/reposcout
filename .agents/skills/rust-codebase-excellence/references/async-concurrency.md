# Async and Concurrency

Read this reference whenever code uses async runtimes, futures, threads, task spawning, channels, locks, atomics, parallelism, timeouts, retries, or graceful shutdown.

## 1. Choose the concurrency model intentionally

Use synchronous code when:

- Work is naturally sequential or bounded by a small number of blocking operations.
- The surrounding codebase is synchronous.
- Async would only move blocking calls behind async syntax.
- Simpler stack traces, cancellation semantics, and resource ownership are more valuable than high concurrency.

Use async when:

- The application already uses an async runtime.
- It must handle many concurrent I/O-bound operations efficiently.
- Protocol or framework APIs are async.
- Coordinated cancellation, timeouts, and concurrency are explicit design requirements.

Use threads or a bounded CPU pool when work is CPU-bound. Async is a concurrency model, not automatic parallel execution.

Do not introduce a second runtime without a strong integration reason. Runtime timers, I/O types, synchronization primitives, and task-local behavior are not generally interchangeable.

### OS threads and data parallelism

- Own and join threads whose completion matters; define how panics, errors, and shutdown are observed.
- Prefer scoped threads when borrowing stack data is the clearest bounded design rather than forcing `'static` ownership.
- Bound thread creation and parallel fan-out. Never map attacker-controlled cardinality directly to one thread per item.
- Account for global or shared worker pools used by parallel libraries; nested parallelism can oversubscribe CPUs and harm latency.
- Keep thread-local state explicit in lifecycle-sensitive code; hidden per-thread caches or runtime handles can complicate tests, shutdown, and memory accounting.
- Define ordering, determinism, and reduction semantics for parallel work, especially for floating-point or externally visible output.

## 2. Task ownership and structured concurrency

Every spawned task needs:

- An owner.
- A reason it may outlive the current call.
- A result or error destination.
- A cancellation policy.
- A shutdown path.
- A bound on how many instances can exist.

Prefer child tasks whose lifetimes are contained by the operation that started them. Join them, collect their results, or manage them through an explicit task set or supervisor.

Detached tasks are acceptable only when the architecture has a durable supervisor and clearly owns their failures. Dropping a join handle must not accidentally turn important work into unobserved background work.

### Spawn checklist

Before spawning, verify:

1. Why concurrency is required instead of `await` or a direct call.
2. Whether borrowed data must become owned and what that costs.
3. How task panics are observed.
4. How cancellation reaches the task.
5. What happens during application shutdown.
6. What limits task count and retained memory.
7. Whether tracing context and request identity propagate.

## 3. Blocking and CPU-heavy work

Do not block an async executor worker with:

- Synchronous filesystem or network operations that may block materially.
- Long CPU loops.
- Compression, parsing, hashing, image work, or cryptographic operations beyond trivial cost.
- Synchronous locks with high contention or long critical sections.
- Child-process waits or blocking SDK calls.

Use the runtime's blocking mechanism or a dedicated bounded worker pool. A blocking pool is not an unlimited escape hatch:

- Bound submitted work.
- Apply backpressure.
- Account for shutdown and cancellation limitations.
- Avoid moving tiny operations off-thread when scheduling cost exceeds the work.
- Separate CPU saturation from I/O latency requirements.

## 4. Locks and shared state

Prefer ownership transfer, immutable sharing, message passing, partitioned state, or actor-like ownership before global shared mutation.

### Synchronous mutex in async code

A standard mutex can be appropriate when:

- The critical section is short and contains no `.await`.
- Contention is low or measured.
- The protected data is ordinary in-memory state.

Release the guard before awaiting. Use an explicit inner scope or `drop(guard)` when necessary, but prefer structure that makes the release obvious.

### Async mutex

An async mutex is appropriate when the protected resource itself must remain exclusively owned across asynchronous operations, such as a stateful I/O resource. It is more expensive and must not become a default wrapper for all shared state.

When holding an async lock across `.await`:

- Keep the operation short and bounded.
- Ensure cancellation cannot leave protocol or application state inconsistent.
- Avoid calling arbitrary user code while locked.
- Analyze lock ordering and reentrancy.
- Do not perform unrelated I/O while holding the guard.

### Read/write locks

Use an `RwLock` only when read concurrency materially helps. Reader-heavy does not guarantee improvement; implementation policy, write starvation, cache behavior, and critical-section cost matter.

### Lock design rules

- Keep critical sections minimal but logically atomic.
- Define a global lock order if multiple locks can be acquired.
- Never hold a lock while invoking callbacks or unknown code unless explicitly designed for it.
- Do not use poisoning as the sole recovery strategy for application invariants.
- Treat lock contention as an architectural signal, not merely a tuning problem.

## 5. Channels and backpressure

Prefer bounded channels unless unbounded growth is a deliberate, externally bounded design.

Define:

- Capacity and why it is sufficient.
- Producer behavior when full: wait, reject, coalesce, drop, or shed load.
- Consumer failure behavior.
- Channel closure semantics.
- Message ownership and size.
- Ordering and delivery guarantees.

An unbounded channel converts overload into memory growth and delayed failure. A large bound can do the same more slowly.

Use channels for ownership transfer and coordination, not to hide a poorly defined shared-state protocol.

## 6. Concurrency limits

Bound fan-out with a semaphore, stream concurrency limit, worker set, or explicit scheduler.

Apply limits to:

- In-flight requests.
- External API calls.
- Database operations.
- Files opened concurrently.
- Blocking tasks.
- CPU-heavy tasks.
- Retries and hedged requests.
- Buffered response or request bodies.

Choose limits from resource budgets and downstream capacity. A concurrency limit without queue or rejection semantics is incomplete.

## 7. Cancellation safety

Dropping a future stops polling that future; work it already started in spawned tasks, blocking calls, remote systems, or the kernel may continue. Review every `.await` inside multi-step operations that mutate state or interact with external systems.

Ask:

- If cancellation occurs here, what state has already changed?
- Can work be safely retried?
- Can data be lost, duplicated, reordered, or partially written?
- Does a lock, permit, transaction, temporary file, or child task get released?
- Must cleanup itself be shielded from cancellation?
- Is an operation documented as cancel-safe by the runtime or library?

Patterns:

- Perform validation before side effects.
- Use transactional or atomic operations when available.
- Persist an idempotency key before retryable external effects.
- Separate prepare, commit, and cleanup phases.
- Use RAII guards for in-process cleanup.
- Explicitly complete or roll back protocol frames and transactions.
- Do not assume `select!` losers have no side effects; their futures are dropped at the current suspension point.

## 8. `select`, races, and fairness

For selection among futures:

- Know which branches are cancel-safe.
- Define priority and fairness instead of assuming it.
- Avoid repeatedly recreating a non-cancel-safe future inside a loop.
- Pin or retain stateful futures when required by the API.
- Handle the case where all inputs terminate or channels close.
- Ensure a frequently ready branch cannot starve maintenance or shutdown work.

Racing duplicate operations can reduce latency but may multiply load and side effects. Use it only with idempotent operations and explicit cancellation or deduplication.

## 9. Timeouts, deadlines, and retries

### Deadlines versus per-attempt timeouts

Prefer propagating an absolute or remaining deadline across layers. Independent per-layer timeouts can exceed the caller's total budget.

A timeout must define what happens to the underlying operation. Timing out the waiter does not necessarily stop blocking work, remote work, or a spawned task.

### Retry policy

Retry only when:

- The failure is classified as transient.
- The operation is idempotent or protected by an idempotency mechanism.
- The total deadline permits another attempt.
- Backoff and jitter avoid synchronization storms.
- Retry count and concurrent retry volume are bounded.

Do not retry validation failures, authentication failures, deterministic conflicts, or unknown side effects blindly.

Include attempt count and final classification in diagnostics without logging sensitive payloads.

## 10. Shutdown

Graceful shutdown is a protocol:

1. Stop accepting new work.
2. Signal cancellation to owned tasks.
3. Close or stop producers.
4. Drain or reject queued work according to policy.
5. Wait for bounded completion.
6. Flush durable state and observability data where required.
7. Force termination after an explicit deadline.

Handle repeated shutdown signals and shutdown during partial startup. Do not wait forever for an uncooperative task.

A resource-owning service should expose an explicit `run`, `shutdown`, or task ownership model rather than relying only on `Drop` for async cleanup, because destructors cannot await.

## 11. `Send`, `Sync`, and task boundaries

- `Send` means a value may be moved to another thread safely.
- `Sync` means shared references may be used across threads safely; equivalently, `&T` is `Send` when `T` is `Sync`.
- Async runtimes may require spawned futures and captured values to be `Send + 'static` when tasks can move between worker threads.
- A local executor can run non-`Send` futures, but that is an architectural choice with placement constraints.
- `Arc<T>` provides shared ownership, not automatic synchronization of `T`.
- `Rc`, `RefCell`, and many guards are intentionally not suitable for cross-thread transfer.

Do not add `unsafe impl Send` or `unsafe impl Sync` to satisfy a compiler error without proving all internal invariants and thread interactions.

## 12. Atomics and lock-free code

Use atomics only for simple, well-specified shared state or after evidence that locks are inadequate.

For every atomic protocol, document:

- The invariant protected.
- Which atomic synchronizes with which operation.
- Why each memory ordering is sufficient.
- What non-atomic data becomes visible.
- ABA, overflow, wraparound, and shutdown behavior.

Defaulting everything to `SeqCst` may be correct but can hide an undefined protocol; weakening ordering for speed without proof can make the protocol incorrect and may invalidate unsafe-code assumptions. Prefer established primitives over custom lock-free structures.

Use Loom or an equivalent model checker for nontrivial synchronization when the repository adopts it. Ordinary stress tests cannot exhaust schedules.

## 13. Async traits and object boundaries

Public async trait design must decide:

- Whether returned futures must be `Send`.
- Whether the trait must support `dyn Trait`.
- Whether allocation and boxed futures are acceptable.
- Whether the trait is public to downstream implementors.
- Which runtime-specific types leak into the API.

Follow the repository's established approach. Do not introduce a macro dependency or boxed-future convention solely because one method happens to be async.

## 14. Concurrency testing

Test:

- Task success, task error, and task panic handling.
- Cancellation at meaningful suspension points.
- Queue saturation and backpressure.
- Channel closure from either side.
- Timeout and retry budgets.
- Shutdown with idle, active, stuck, and partially initialized tasks.
- Lock ordering and concurrent updates.
- Duplicate, delayed, and reordered messages where the protocol permits them.

Use paused or fake time instead of sleeps. Keep tests bounded and deterministic. When model checking, reduce the state space and assert invariants rather than relying on timing.

## 15. Common failures to reject

- `tokio::spawn` used to avoid a borrow or ownership redesign.
- Fire-and-forget writes whose errors disappear.
- An unbounded `mpsc` channel on an external request path.
- A `Mutex` guard held across `.await` by accident.
- A timeout wrapper around non-cancelable blocking work with the assumption that work stopped.
- Retrying a non-idempotent operation after an unknown outcome.
- `Arc<Mutex<HashMap<...>>>` as the default architecture for all state.
- An async API whose implementation performs only blocking calls.
- A task loop with no shutdown signal or closed-channel exit.
- An `unsafe impl Send/Sync` added without a written proof.

## Async completion checklist

- Concurrency model is justified and runtime-consistent.
- Every task is owned; task count and queues are bounded.
- Blocking and CPU-heavy work is isolated appropriately.
- Lock scope and order are explicit.
- Cancellation, timeout, retry, and shutdown semantics are correct.
- `Send` and `Sync` assumptions match concrete types.
- Tests cover overload, cancellation, closure, and shutdown—not only success.
