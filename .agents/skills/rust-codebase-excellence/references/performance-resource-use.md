# Performance and Resource Use

Read this reference when a change may affect latency, throughput, allocation rate, memory footprint, I/O volume, startup time, binary size, compile time, or behavior under load.

Performance work is an empirical engineering activity. Preserve correctness and operational safety first, then optimize the bottleneck that measurements identify.

## 1. Establish the performance contract

Before optimizing, define what matters:

- Latency: median, tail latency, deadline, or worst-case bound.
- Throughput: operations, requests, messages, or bytes per unit time.
- Memory: steady-state resident set, peak use, per-request growth, or allocator pressure.
- CPU: total work, single-thread latency, parallel scalability, or energy budget.
- I/O: syscall count, bytes transferred, round trips, batching, or storage amplification.
- Startup and shutdown time.
- Binary size or artifact download size.
- Compile time and incremental developer feedback.
- Target hardware, operating system, architecture, build profile, feature set, and representative workload.

Do not optimize against an unspecified goal. A change that improves throughput while damaging tail latency, memory, fairness, or readability may be a regression.

## 2. Measure before and after

Use a repeatable baseline:

1. Record the exact commit, toolchain, target, features, profile, hardware, and relevant environment.
2. Warm caches when production behavior is warm; measure cold behavior separately when it matters.
3. Use representative input distributions, including pathological but valid cases.
4. Run enough samples to distinguish signal from noise.
5. Compare distributions or confidence intervals, not only a single best run.
6. Change one meaningful variable at a time.
7. Validate output equivalence and resource limits after the optimization.

Prefer production traces, profiles, and realistic benchmarks over intuition. Microbenchmarks are useful for isolated mechanisms but do not establish end-to-end impact by themselves.

## 3. Build profiles matter

- Do not infer production performance from an unoptimized development build.
- Benchmark the profile and target actually shipped, usually `--release` or a repository-defined profile.
- Preserve overflow checks, debug assertions, panic strategy, LTO, codegen units, stripping, and target CPU settings unless the project intentionally changes them.
- Treat `-C target-cpu=native` as a deployment compatibility decision, not a universal speed flag.
- Do not compare builds produced with materially different compiler versions or profile settings without stating that limitation.

## 4. Profiling workflow

Choose a tool appropriate to the suspected resource:

- CPU sampling or tracing for time attribution.
- Allocation profiling for allocation count, size, and lifetime.
- Heap or resident-memory profiling for retained memory and peaks.
- I/O, syscall, scheduler, and lock profiling for wait-heavy systems.
- Flame graphs or call trees for orientation, followed by source-level confirmation.
- Application tracing and metrics for production-only behavior.

Interpret profiles carefully:

- Inclusive time identifies expensive call paths; self time identifies expensive bodies.
- A hot function may be called too often rather than be locally inefficient.
- An allocation site may be cheap but retain memory through a long-lived owner.
- Lock contention, queueing, and executor starvation may appear as low CPU utilization.
- Compiler inlining and monomorphization can obscure source-level frames.

## 5. Algorithm and data structure first

Prefer asymptotic and architectural improvements over instruction-level tuning:

- Avoid repeated linear scans when indexing is justified.
- Avoid accidental quadratic concatenation, insertion, parsing, or deduplication.
- Stream or paginate data that need not be resident at once.
- Batch work when it reduces fixed overhead without violating latency or memory limits.
- Use data structures that match access patterns, ordering needs, mutation frequency, and cardinality.
- Avoid sorting when only a minimum, maximum, partition, or top-k result is required.
- Cache only when invalidation, memory bounds, hit rate, and consistency semantics are defined.

Do not replace a simple structure with a more complex one for hypothetical scale.

## 6. Allocation and ownership

Allocation optimization must preserve understandable ownership.

Consider, in order:

1. Eliminate work or data that is not needed.
2. Reuse an existing buffer when ownership and lifetime make reuse clear.
3. Preallocate from a trustworthy size estimate with a sensible upper bound.
4. Borrow or slice existing storage when the result cannot outlive it.
5. Batch small allocations when profiling shows allocator overhead or fragmentation.
6. Consider specialized containers only after measuring their complete cost.

Guidance:

- `Vec::with_capacity` and `String::with_capacity` are useful when a reasonable bound is known; hostile lengths must not drive unbounded allocation.
- `SmallVec` stores its inline capacity inside the value and spills to the heap only beyond that capacity. A large inline array makes every value large; use it only for a measured small-cardinality distribution.
- `Box<[T]>` is useful for fixed-length heap-owned sequences that no longer need spare capacity.
- `Cow` is an API ownership tool, not a generic allocation optimization.
- Interning can reduce duplication but adds global or arena lifetime, synchronization, and retention concerns.
- Arenas can simplify bulk lifetimes and improve locality, but cannot replace resource bounds and may retain all allocations until the arena is dropped.
- Do not return references tied to caches or arenas unless the lifetime model is stable and obvious to callers.

## 7. Copies, clones, and moves

Rust moves are not automatically expensive, and stack size alone does not define API quality.

- A move of a `Vec`, `String`, `Box`, or `Arc` usually moves a small handle, not the backing allocation.
- A `Clone` may be cheap or expensive depending on the type and sharing semantics.
- Passing a small `Copy` type by value is often clear; borrowing can be equally appropriate when it matches the API.
- Large aggregates may benefit from indirection, but measure ABI and locality effects before redesigning public types.
- Avoid cloning solely to satisfy the borrow checker before checking whether scopes, ownership transfer, or API shape can express the true lifetime.
- Keep a clone when it makes ownership explicit and the cost is irrelevant to the contract.

## 8. Strings, bytes, and parsing

- Parse once at trust boundaries into a representation suited to internal use.
- Avoid repeated UTF-8 validation, normalization, case folding, or numeric parsing in hot paths.
- Use `&[u8]` for opaque bytes and `&str` for validated UTF-8; do not convert repeatedly without reason.
- Avoid constructing temporary `String` values solely for lookup when borrowed lookup is supported.
- For incremental protocols, retain incomplete input safely and cap frame or message sizes.
- Prefer established parsers for complex or security-sensitive formats.
- Zero-copy parsing is worthwhile only when retained input lifetimes, fragmentation, and memory ownership remain manageable.

## 9. Iteration and bounds checks

- Clear iterator chains and clear loops can both optimize well.
- Choose the form that exposes invariants and minimizes accidental repeated work.
- Avoid collecting an iterator merely to iterate again unless materialization is required.
- Fuse transformations when it remains readable and actually removes work or allocation.
- Do not use unchecked indexing or raw pointers merely to avoid presumed bounds checks. Inspect generated code and benchmark first.
- Prefer slice operations, iterators, and chunk APIs that communicate bounds to the compiler safely.

## 10. Layout, locality, and representation

Data layout can dominate performance at scale:

- Keep frequently accessed data compact and contiguous when access patterns benefit.
- Separate hot and cold fields when large cold data harms cache density.
- Avoid pointer-heavy graphs for data naturally processed sequentially.
- Consider structure-of-arrays versus array-of-structures only with a concrete access pattern and benchmark.
- Reordering public struct fields, changing enum representation, or adding `repr(...)` can affect ABI, serialization assumptions, size, and SemVer expectations; treat it as a contract decision.
- Niche optimization and enum size are compiler properties, not APIs to rely on without explicit representation guarantees.
- Avoid packed representations unless required by an external format; unaligned access and references require special care.

## 11. Hashing, maps, and adversarial inputs

- Select a map or set based on ordering, lookup pattern, memory, determinism, and threat model.
- Do not replace the standard randomized hash builder with a faster non-cryptographic alternative on attacker-controlled keys without a denial-of-service analysis.
- Stable output must not depend accidentally on randomized hash iteration order.
- Reserve capacity from trusted estimates and cap attacker-controlled cardinality.
- For tiny collections, a linear scan may outperform a map and be simpler; measure representative sizes.

## 12. I/O and system calls

- Buffer small reads and writes when the underlying API is unbuffered.
- Avoid flushing after every small write unless durability or protocol semantics require it.
- Batch database or network operations only within transaction, ordering, timeout, and memory constraints.
- Reuse connections through a bounded pool with health, timeout, and backpressure policies.
- Avoid read-to-end for unbounded input.
- Use vectored or zero-copy I/O only when the platform and workload benefit and fallback behavior is correct.
- Account for partial reads, partial writes, interruption, short buffers, and filesystem semantics.

## 13. Async and parallel performance

When async or parallel execution is involved, also apply the concurrency guidance routed by `SKILL.md`.

- Concurrency can hide wait time; it does not reduce total work automatically.
- Bound in-flight operations and queue depth.
- Avoid spawning a task for extremely small work when scheduling overhead dominates.
- Move CPU-heavy work off executor threads and bound the worker pool.
- Minimize lock hold time and avoid lock convoys.
- Prefer sharding, ownership transfer, or message passing when shared-state contention is the bottleneck.
- Parallel iterators or work stealing help only when tasks are large enough and independent enough.
- Check fairness and tail latency; maximizing throughput can starve low-volume work.

## 14. SIMD, intrinsics, and platform specialization

Use architecture-specific optimization only after portable code is measured and insufficient.

- Keep a correct portable implementation and test specialized implementations against it.
- Detect runtime CPU features where binaries run on heterogeneous hardware.
- Isolate `unsafe` intrinsics behind a small safe API with documented preconditions.
- Benchmark end-to-end, including dispatch and alignment overhead.
- Do not assume vectorization; inspect compiler output when it is material.

## 15. Memory behavior and resource bounds

- Define bounds for request bodies, decompressed data, recursion, collections, queues, caches, task count, open files, and retained history.
- Release large temporary buffers promptly when peak memory matters; do not churn allocations without evidence.
- Watch for logical leaks through caches, registries, channels, detached tasks, `Arc` cycles, and ever-growing metrics labels.
- Break reference cycles deliberately; `Weak` is appropriate only when the ownership graph genuinely has non-owning edges.
- Account for allocator fragmentation and per-thread caches when RSS differs from live allocation.
- A memory optimization that increases unbounded recomputation or latency may be unacceptable.

## 16. Binary size

When binary size is a real constraint:

- Measure stripped release artifacts for the shipped targets.
- Inspect which crates, features, symbols, and monomorphizations contribute.
- Disable unused dependency features deliberately.
- Avoid broad generic instantiation across many types where dynamic dispatch or a shared non-generic core is suitable.
- Consider profile settings such as LTO, codegen units, panic strategy, optimization for size, and stripping only after evaluating compile time, runtime, diagnostics, and unwind requirements.
- Do not compromise correctness or maintainability for negligible size savings.

## 17. Compile time

Compile-time work affects engineering throughput:

- Distinguish clean, incremental, check, test, and release build costs.
- Avoid unnecessary feature unification and dependency duplication.
- Keep proc macros and build scripts focused; they run during compilation and can invalidate caches.
- Split crates only when package boundaries, parallel compilation, reusable stable interfaces, or change isolation justify the cost. Excessive crate graphs increase coordination and linking overhead.
- Reduce unnecessary generic and macro expansion in hot compilation paths.
- Do not optimize compile time by weakening required type safety or tests.

## 18. Benchmark design

A benchmark should state:

- The question it answers.
- Inputs and distributions.
- Setup included or excluded from timing.
- Build profile and target.
- Expected sources of noise.
- Correctness checks outside the timed path.
- The practical threshold for accepting a change.

Avoid:

- Benchmarking constant-foldable work without black-boxing inputs and outputs.
- Timing allocation or setup accidentally when the production path reuses state, or excluding it when production pays it.
- Comparing benchmark frameworks or configurations as though results were equivalent.
- Treating a statistically significant but operationally irrelevant difference as a reason for complexity.

## 19. Performance review checklist

Before accepting an optimization, verify:

- The bottleneck was measured on a representative workload.
- The change improves the stated metric, not merely a microbenchmark proxy.
- Correctness, determinism, security, resource limits, and cancellation behavior remain intact.
- Memory and tail-latency effects were considered.
- The code remains maintainable and contains comments only where the non-obvious optimization needs justification.
- A benchmark or regression test protects the important property when practical.
- The performance assumption is documented closely enough to re-evaluate later.

Reject speculative optimization that adds complexity without evidence or a concrete budget.
