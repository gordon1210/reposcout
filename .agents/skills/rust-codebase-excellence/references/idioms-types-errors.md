# Idioms, Types, Ownership, and Errors

Read this reference for implementation-level Rust decisions: ownership, borrowing, cloning, collections, pointer types, traits, generics, state modeling, error APIs, panics, and conversions.

## 1. Ownership follows semantics

Use the signature to state what the function does with data:

| Intent | Typical shape |
|---|---|
| Observe text during the call | `&str` |
| Observe a sequence during the call | `&[T]` |
| Observe a filesystem path | `&Path` |
| Consume or retain a value | `T`, `String`, `Vec<T>`, `PathBuf` |
| Mutate caller-owned state | `&mut T` |
| Accept borrowed input but sometimes return owned data | `Cow<'a, T>` when this is a real API requirement |
| Share immutable ownership in one thread | `Rc<T>` |
| Share ownership across threads | `Arc<T>` when `T` satisfies the required thread-safety bounds |

Rules:

- Prefer borrowing when ownership is not needed, but do not design lifetime-heavy APIs solely to remove an insignificant allocation.
- Passing a small value by reference can be worse ergonomically and no faster. Choose by semantic ownership first; measure ABI or copy cost only when relevant.
- Accept owned values when the function queues, spawns, stores, caches, or returns them without a useful borrow relationship.
- Accept `impl Into<String>` or similar conversion parameters selectively for ownership-taking constructors. Excessive generic conversion parameters can worsen type inference, compile time, diagnostics, and API predictability.
- Return borrowed data only when the lifetime relationship is natural and useful to callers.

## 2. Cloning

A clone is correct when independent ownership is required. It is suspicious when it hides an unclear ownership design.

Before cloning, ask:

1. Must both owners outlive this point?
2. Could the value be moved instead?
3. Could the operation borrow for the required duration?
4. Is the clone cheap and outside a relevant hot path?
5. Would avoiding it make the API or lifetime structure materially harder to understand?

Do not:

- Clone reflexively to satisfy the borrow checker.
- Add `Clone` to a type solely to make calling code convenient.
- Clone inside loops, retries, request paths, or large-buffer operations without understanding cost.
- Replace a clear cheap clone with `Rc`, `Arc`, or interior mutability unless shared ownership is actually required.

For expensive or security-sensitive types, make cloning deliberate and visible. Consider omitting `Clone` when duplication itself is dangerous or semantically invalid.

## 3. Collections and allocation

- Accept slices instead of `&Vec<T>` unless vector capacity or vector-specific operations are part of the contract.
- Return `Vec<T>` when ownership and contiguous storage are useful; do not return iterators merely to appear lazy if the implementation must allocate everything anyway.
- Avoid intermediate `collect()` when a streaming iterator is clearer and materially reduces allocation.
- Collect deliberately when multiple passes, sorting, ownership, stable snapshots, or simpler error handling justify it.
- Preallocate with `with_capacity` only when a useful size estimate exists.
- Use `Box<[T]>` for fixed-length owned slices when spare capacity is unnecessary.
- `SmallVec` stores its inline capacity inside the value and spills only after that capacity is exceeded. Large inline capacities enlarge every value; use it only when the common small-size distribution is known or measured.
- Choose `HashMap`, `BTreeMap`, `IndexMap`, or specialized structures from ordering, hashing, determinism, range-query, and performance requirements—not habit.
- Never expose nondeterministic hash iteration order as a stable output contract.

## 4. Strings, paths, and bytes

- Use `String` and `&str` for valid UTF-8 text.
- Use `OsString` and `&OsStr` for operating-system strings that may not be UTF-8.
- Use `PathBuf` and `&Path` for filesystem paths.
- Use `Vec<u8>` and `&[u8]` for arbitrary bytes.
- Do not round-trip paths through lossy UTF-8 when exact identity matters.
- Distinguish text encoding validation from parsing. Validate at the boundary and preserve original bytes when required.
- Avoid repeated formatting and reparsing to convert structured values.

## 5. Numeric and temporal types

- Use domain types such as `Duration`, `Instant`, non-zero integers, and newtypes for units rather than naked integers where confusion is plausible.
- Use `TryFrom` or explicit range checks for narrowing and signedness changes.
- Choose checked, saturating, or wrapping arithmetic from domain semantics and make the choice visible.
- Avoid `as` for potentially lossy numeric conversions unless the truncation or wrapping behavior is explicitly intended and documented.
- Treat lengths, offsets, capacities, and allocation sizes as untrusted when derived from input.
- Avoid mixing wall-clock time with monotonic elapsed-time measurement.
- Define timestamp timezone, epoch, precision, and serialization explicitly.

## 6. Modeling state and invariants

Use the least complex representation that prevents meaningful bugs:

1. Plain fields for unconstrained data.
2. Newtype with a validating constructor for one value invariant.
3. Enum for a closed runtime state set.
4. Struct variants or enum payloads when states carry different data.
5. Typestate when invalid operation sequences should be impossible at compile time for a stable, important protocol.

Prefer:

```rust
struct ClosedFile {
    path: PathBuf,
}

struct OpenFile {
    handle: std::fs::File,
}
```

over a phantom-state wrapper that stores `Option<File>` in every state and relies on `unreachable!()` to assert the option is populated.

Typestate is justified when:

- Misordered calls create serious correctness or safety failures.
- The state graph is small and stable.
- The API is reused enough to repay generic complexity.
- State transitions consume one type and produce another with state-specific data.

Use an enum when state changes are dynamic, need persistence or inspection, or callers would otherwise fight generic type changes.

## 7. Traits, generics, and dispatch

### Add a trait when it owns behavior

A trait is useful for:

- Multiple real implementations.
- A stable domain capability.
- Runtime plugin or strategy selection.
- Dependency inversion across a meaningful boundary.
- A test seam that replaces an expensive or nondeterministic external system.

A trait is not justified merely because a struct has methods or a future implementation is imaginable.

### Static dispatch

Use generics or `impl Trait` when:

- Compile-time specialization and inlining matter.
- The implementation type is naturally part of composition.
- The trait is not object-safe.
- Callers benefit from zero-cost abstraction and code-size growth is acceptable.

### Dynamic dispatch

Use `dyn Trait` when:

- Implementation selection happens at runtime.
- Heterogeneous implementations share storage.
- ABI-like or plugin boundaries need erased concrete types.
- Reducing monomorphization and compile-time or binary-size cost matters.
- A simpler non-generic public API is more valuable than possible inlining.

Keep ownership explicit: `&dyn Trait`, `Box<dyn Trait>`, and `Arc<dyn Trait + Send + Sync>` express different lifetimes and ownership.

### Trait API cautions

- Adding a required method to a public trait is breaking for external implementors.
- Blanket implementations can conflict with downstream code and restrict future evolution.
- Auto traits such as `Send` and `Sync` are observable API properties.
- Async methods in public traits commit to future and `Send` behavior; follow the repository's established pattern and intended object-safety requirements.
- Avoid a generic parameter when an associated type better expresses one implementation-specific type.

## 8. Smart pointers and interior mutability

| Type | Use |
|---|---|
| `Box<T>` | Unique heap ownership, recursive types, or deliberate indirection |
| `Rc<T>` | Shared ownership within one thread |
| `Arc<T>` | Atomic shared ownership; not automatic interior thread safety |
| `Cell<T>` | Copy-oriented interior mutation in one thread |
| `RefCell<T>` | Dynamically checked borrowing in one thread |
| `Mutex<T>` | Exclusive synchronized access; poisoning semantics depend on implementation |
| `RwLock<T>` | Read/write synchronization when workload and implementation make it beneficial |
| Atomics | Small lock-free state with a rigorously defined memory-ordering protocol |

Thread-safety traits are conditional:

- `T: Sync` means `&T` is safe to send between threads.
- `&T: Send` exactly when `T: Sync`.
- `Box<T>` is `Send` when `T: Send` and `Sync` when `T: Sync`.
- `Arc<T>` requires appropriate `Send` and `Sync` bounds on `T` for cross-thread transfer and sharing.
- Wrapping a non-thread-safe value in `Arc` does not make the inner value thread-safe.
- Lock and guard types have their own conditional `Send` or `Sync` implementations; verify the concrete type rather than relying on a table from memory.

Use interior mutability only when mutation through shared access is intrinsic to the design. Do not use it to avoid restructuring ordinary ownership.

`Pin` is a contract about whether a pointee may be moved after pinning; it is not a general ownership or heap-allocation tool and does not make an unsound self-referential design safe automatically. Use it only when an API or invariant genuinely requires pinning.

`PhantomData` affects variance, drop checking, and auto-trait behavior. Do not add it as a decorative state marker without checking those consequences.

## 9. Iterators and loops

Both are idiomatic.

Prefer iterator adapters when they express a clear data transformation. Prefer a loop when it improves:

- Early exits or multi-branch control flow.
- Stateful mutation.
- Error handling.
- Debuggability.
- Borrow-checker clarity.
- Performance after measurement.

Avoid long chains that require comments to explain control flow. Avoid manual index loops when direct iteration removes bounds and indexing mistakes.

Use consuming iteration when ownership should move, shared iteration for observation, and mutable iteration for in-place mutation. Do not choose `.iter()` or `.into_iter()` from a blanket rule; choose the ownership semantics.

## 10. RAII and destruction

Use RAII guards for cleanup that must happen on every scope exit, including early returns and unwinding.

- `Drop::drop` cannot return an error or await. Provide an explicit `close`, `finish`, `commit`, or `shutdown` operation when callers must observe completion failure.
- Destructors should not panic; a second panic during unwinding aborts the process.
- Do not rely on `Drop` alone for async cleanup, remote confirmation, or durable flushing.
- Understand field and local drop order when correctness depends on one resource outliving another; prefer structuring ownership so the order is evident.
- Use `ManuallyDrop`, `mem::forget`, and raw deallocation only with a complete ownership and panic-safety analysis.
- A guard that rolls back on drop must define what happens if rollback itself can fail.

## 11. Error taxonomy

Classify errors before choosing a type:

- **Input/domain error:** caller can correct input or state.
- **Transient external error:** retry may help under a defined policy.
- **Permanent external error:** configuration, permission, compatibility, or missing resource.
- **Conflict:** optimistic concurrency, duplicate state, or precondition failure.
- **Cancellation or timeout:** distinct operational outcome, not necessarily an internal failure.
- **Bug/invariant violation:** panic or process-fatal behavior may be appropriate.

Do not collapse materially different actions into one opaque string.

## 12. Library errors

A public library error should:

- Be stable enough for callers to classify only what they need.
- Preserve lower-level causes with `Error::source` where useful.
- Avoid exposing every internal dependency error as a public variant.
- Carry actionable structured data without leaking secrets.
- Document which operations can produce which categories.
- Remain reasonably evolvable; exhaustive public enums create compatibility commitments.

`thiserror` is a convenient derive tool, not a requirement. A small library can implement `Display` and `Error` directly. Choose based on existing dependencies and complexity.

Consider separate error types per coherent subsystem rather than one giant application-wide enum.

## 13. Application errors

At an application boundary:

- Add context that identifies the failed operation and relevant non-sensitive identifiers.
- Convert domain errors into user-facing messages, protocol status, exit codes, metrics, or retry decisions once, at the correct boundary.
- Avoid logging the same error at every layer.
- Preserve the source chain for diagnostics.

Report-oriented crates such as `anyhow` or `eyre` can be appropriate in binaries and orchestration layers. Do not let opaque reports replace typed errors where code must branch on failure.

## 14. Panic, `unwrap`, and `expect`

Use `Result` by default for failures outside the function's control. Panic when continuing would indicate a programming error or an intentionally fatal invariant.

Acceptable examples include:

- Tests and test fixtures.
- A compile-time or locally constructed value whose validity is immediately evident.
- An internal map lookup guaranteed by an adjacent insertion and protected from future drift by structure or assertion.
- Process startup when a mandatory static configuration is intentionally unrecoverable and the failure message is actionable.

Unacceptable examples include:

- User, network, filesystem, database, environment, or deserialization input.
- Channel closure or task failure that is operationally possible.
- Indexing based on external lengths.
- “This should never happen” without an enforceable invariant.

An `expect` message should describe the invariant:

```rust
let header = headers
    .get("content-type")
    .expect("content-type was inserted during request construction");
```

Do not write messages such as `expect("get content type")`, which add no reasoning.

## 15. Conversion and parsing

- Use `From` for infallible, lossless, obvious conversions.
- Use `TryFrom` for validation or possible failure.
- Use `AsRef` for cheap borrowed views where generic flexibility benefits callers.
- Use `Borrow` only when equality, ordering, and hashing semantics match the owned type, especially for collection lookup.
- Implement `FromStr` for canonical textual parsing.
- Do not use `From` for semantically surprising unit changes, lossy transformations, or expensive I/O.
- Keep parsing separate from side effects when possible so it can be tested deterministically.

## 16. Comments and naming

- Names should expose domain intent and units.
- Comments explain rationale, invariants, safety, non-obvious complexity, protocol constraints, or workarounds.
- Do not narrate syntax.
- Remove comments that became false after a refactor.
- Make TODOs actionable with the missing condition or tracking reference; do not leave vague future intentions.

## Implementation completion checklist

- Ownership in signatures matches retention and mutation.
- Clones and allocations are intentional.
- State representation eliminates the right class of bugs without excess machinery.
- Traits and generics own real variation.
- Smart-pointer and thread-safety assumptions are correct for concrete types.
- Errors preserve actionable classification and causes.
- Panics are limited to documented invariants or deliberate fatal boundaries.
- Conversions are explicit about loss and validation.
