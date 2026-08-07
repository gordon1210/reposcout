# Security, Dependencies, and Unsafe Rust

Read this reference for untrusted input, command or filesystem boundaries, secrets, denial-of-service resistance, dependency and build-time supply chain, `unsafe`, FFI, native code, and security review.

## 1. Define the trust boundary

Before implementing security-sensitive code, identify:

- Which data is attacker-controlled, tenant-controlled, user-controlled, or merely malformed by accident.
- Which operations cross process, network, filesystem, database, privilege, or language boundaries.
- Which resources an attacker can cause the program to consume.
- Which identities, permissions, and tenancy constraints apply.
- Which data is confidential, integrity-sensitive, regulated, or security-relevant.
- What happens after partial failure, retry, restart, or concurrent execution.

Validate at the boundary, convert to an internal validated representation, and avoid repeatedly reinterpreting raw input deeper in the system.

## 2. Input validation and resource limits

Validation must cover both meaning and cost.

Check as applicable:

- Length, count, nesting depth, recursion, and aggregate decoded size.
- Numeric range, overflow, underflow, and allocation calculations.
- Encoding, normalization, duplicate fields, unknown fields, and canonical form.
- Path roots, traversal, symlinks, race windows, reserved names, and platform semantics.
- URL scheme, host, port, redirect policy, DNS behavior, and destination allowlists.
- Archive expansion ratio, entry count, link handling, and extraction destination.
- Compression, parsing, regex, and algorithmic complexity.
- Request body size, decompression limits, timeouts, and concurrency.
- Database query cardinality, pagination bounds, and sort/filter complexity.

Do not allocate directly from an untrusted declared length without a verified upper bound. Do not parse recursively without a depth limit when input controls nesting.

## 3. Filesystem safety

- Treat path validation and file opening as one security-sensitive operation; a validated path can change before use.
- Prefer capability-style access rooted at an already-open directory when the threat model requires race resistance.
- Canonicalization alone is not a complete traversal defense and can fail for paths that do not yet exist.
- Decide whether symlinks are allowed and enforce the policy at the operation boundary.
- Create temporary files securely and atomically; do not guess predictable names.
- Set permissions explicitly for sensitive files instead of relying only on ambient defaults.
- Use atomic replace patterns for durable configuration or state updates where partial writes are unacceptable.
- Bound file size, file count, and total extraction or traversal work.
- Preserve non-UTF-8 paths when exact filesystem identity matters.

## 4. Process and command execution

- Prefer direct argument APIs over shell command strings.
- Never concatenate untrusted input into a shell command.
- Treat executable lookup through `PATH`, current directory, environment, and working directory as part of the trust model.
- Pass an explicit environment allowlist for privileged or sensitive subprocesses.
- Bound runtime, output size, stdin size, and process count.
- Define termination behavior for child process trees, not only the immediate child.
- Capture and report stderr safely without leaking secrets or allowing unbounded memory use.
- Treat command exit status, signal termination, and spawn failure as distinct outcomes where callers need them.

## 5. Network and protocol boundaries

- Apply connect, read, write, idle, and total deadlines appropriate to the protocol.
- Limit redirects and revalidate every redirect target.
- Defend against server-side request forgery when users influence destinations; account for DNS rebinding and private or link-local ranges according to the threat model.
- Verify TLS identities and do not disable certificate validation outside tightly controlled test fixtures.
- Bound headers, frames, messages, decompressed bodies, and concurrent streams.
- Validate protocol state transitions, sequence numbers, lengths, and checksums before trusting payloads.
- Make retry semantics idempotent or use idempotency keys.
- Avoid logging credentials, authorization headers, session tokens, signed URLs, or raw sensitive payloads.

## 6. Authentication, authorization, and tenancy

- Authentication establishes identity; authorization must be checked for the concrete resource and operation.
- Perform authorization server-side and close to the resource boundary.
- Do not rely on identifiers being unguessable as access control.
- Include tenant or ownership scope in queries and updates, not as a later filtering step.
- Recheck authorization for state-changing operations where relevant; cached decisions need explicit invalidation semantics.
- Distinguish unauthenticated, unauthorized, not-found, and conflict responses according to information-disclosure policy.
- Keep audit events structured and tamper-aware, but do not put secrets into them.

## 7. Secrets and sensitive data

- Do not hard-code secrets or commit them to fixtures, logs, snapshots, panic messages, or generated artifacts.
- Avoid deriving `Debug` or `Serialize` for secret-bearing types unless the representation is intentionally redacted.
- Use dedicated redacted wrappers or custom formatting for credentials and tokens.
- Minimize secret lifetime and copies. Understand that ordinary Rust memory is not automatically zeroized and the optimizer may affect manual clearing.
- Add a zeroization dependency only when the threat model and data lifecycle justify it.
- Compare authentication tags and secrets with an established constant-time primitive when timing leakage matters.
- Keep key material and algorithms out of application-specific custom cryptographic code.
- Separate user-facing errors from detailed internal diagnostics.

## 8. Serialization and deserialization

- Treat deserialization as input processing, not validation completion.
- Enforce semantic invariants after structural parsing.
- Define duplicate-field, unknown-field, and version behavior deliberately.
- Bound nesting, collection lengths, strings, byte arrays, and aggregate allocation.
- Avoid untagged or highly ambiguous formats where attacker-controlled input can trigger expensive backtracking.
- Use explicit versions for persisted or networked formats that must evolve.
- Test old fixtures, future unknown fields, malformed input, and migration behavior.
- Never deserialize directly into a type whose constructor normally enforces invariants unless deserialization enforces the same rules.

## 9. Dependency review

A Rust dependency can add:

- Runtime code.
- Build scripts executed on the host.
- Proc macros executed in the compiler process.
- Native libraries and toolchain requirements.
- Network, filesystem, environment, and platform behavior.
- New licenses, sources, and transitive maintenance risk.
- MSRV and feature changes.

Before adding one:

1. Confirm a dependency is better than clear local code for this problem.
2. Inspect maintenance, release cadence, ownership, documentation, security advisories, and issue history.
3. Inspect default and optional features.
4. Review direct dependencies, build dependencies, proc macros, native code, and source origin.
5. Confirm MSRV and target compatibility.
6. Check license policy.
7. Assess whether its types leak into public API.
8. Pin git revisions when git dependencies are unavoidable; prefer registry releases for reproducibility.
9. Review the lockfile diff and `cargo tree` output.
10. Add only the features needed, but do not disable defaults without understanding the consequences.

Do not add a dependency merely because it is popular. Do not reimplement cryptography, parsers, codecs, TLS, authentication protocols, or concurrency primitives merely to reduce dependency count.

## 10. Supply-chain tooling

Use repository-approved tooling:

- `cargo audit` checks lockfiles against RustSec advisories.
- `cargo deny` can enforce advisories, licenses, banned or duplicate crates, and allowed sources.
- `cargo tree` explains dependency paths and feature activation.
- Source and artifact scanners may complement Rust-specific tooling for deployed binaries and containers.

Rules:

- Do not auto-ignore advisories. Record the exact advisory, affected code path, compensating control, owner, and expiry or review condition.
- Distinguish vulnerable, unmaintained, yanked, and informational findings.
- Check whether a vulnerable feature or target is actually built, but do not use “not currently reachable” as a permanent substitute for remediation.
- Treat source changes, new git dependencies, new build scripts, and new proc macros as elevated review events.
- Pin CI actions and downloaded tools according to repository policy.
- Generate or retain SBOM/provenance data when deployment or compliance requires it.

## 11. Executing Cargo in untrusted repositories

`cargo check`, `cargo build`, `cargo test`, `cargo clippy`, and documentation builds can execute build scripts and proc macros. They may also invoke native toolchains.

For unknown or untrusted code:

- Inspect manifests, source origins, build dependencies, proc macros, `.cargo/config*`, environment hooks, and wrapper settings first.
- Run in a sandbox with no host secrets, minimal filesystem access, restricted network, and resource limits.
- Do not mount Docker sockets, SSH agents, cloud credentials, package-manager credentials, or broad home directories.
- Treat test binaries as arbitrary executable code.
- Do not run `cargo install` from the repository or instructions without explicit review and approval.

Static inspection is safer but cannot replace compilation forever; move to a controlled execution environment when validation is required.

## 12. Safe Rust first

Safe Rust prevents memory-safety violations only when all unsafe code and external components uphold their contracts. Safe code can still contain logic flaws, denial of service, deadlocks, races, authorization bugs, and data loss.

For crates that intentionally prohibit unsafe Rust, enforce the `unsafe_code` lint at the repository-chosen level and verify how generated code, macros, tests, and target-specific modules interact with that policy.

Use `unsafe` only when required for:

- FFI or platform interfaces.
- A proven performance or memory-layout need unavailable in safe Rust.
- Implementing a safe abstraction or low-level primitive with documented invariants.

Do not use `unsafe` merely to:

- Avoid a clone without measurement.
- Extend a lifetime the type system rejects.
- Circumvent aliasing or thread-safety errors.
- Silence initialization or bounds checks.
- Recreate a standard library primitive.

## 13. Unsafe block discipline

Every unsafe operation requires a proof obligation.

- Keep unsafe blocks minimal; do not wrap an entire function when only one expression is unsafe.
- Enable or honor `unsafe_op_in_unsafe_fn` so unsafe functions still mark individual unsafe operations.
- Precede each block with `// SAFETY:` describing the concrete invariant that makes the operations valid.
- State who establishes the invariant and how it remains true.
- Keep unsafe code in a small module with a safe external interface.
- Avoid exposing raw pointers or unchecked constructors unless callers genuinely need them.
- Use debug assertions only as diagnostics; they do not establish release-mode safety.
- Do not rely on current compiler layout or optimization accidents unless the language or type contract guarantees them.

Weak safety comment:

```rust
// SAFETY: pointer is valid.
```

Useful safety comment:

```rust
// SAFETY: `ptr` was produced by `Vec::as_mut_ptr`, `len <= capacity`, the
// allocation remains alive for this scope, and no other reference accesses
// these elements until the returned slice is dropped.
```

The comment must match the exact code after every refactor.

## 14. Public unsafe APIs

For every public `unsafe fn`, unsafe trait, or unsafe method, document a `# Safety` section containing all caller obligations:

- Pointer validity, alignment, provenance, and allocation origin.
- Initialization and valid bit patterns.
- Aliasing and exclusivity.
- Lifetime and ownership transfer.
- Length, capacity, and bounds relationships.
- Thread and reentrancy constraints.
- ABI, unwind, and callback requirements.
- State-machine preconditions.

If callers cannot realistically verify the obligations, the API is too unsafe; provide a safer wrapper.

## 15. Unsafe trait implementations

`unsafe impl Send`, `Sync`, allocator traits, and other unsafe traits assert global properties.

- Explain why every field and reachable state satisfies the trait contract.
- Account for generic parameters with correct bounds.
- Consider drop behavior, callbacks, interior mutability, aliasing, and thread handoff.
- Do not assume `Arc` or a lock wrapper repairs an unsound inner type automatically.
- Add compile-time positive and negative assertions where useful.
- Test under concurrency model tools when the implementation defines synchronization behavior.

## 16. Common unsafe hazards

Review explicitly for:

- Creating references from null, dangling, misaligned, or invalid pointers.
- References to uninitialized or invalid-value data.
- Aliasing `&mut` or mutation behind shared references without valid interior-mutability machinery.
- Incorrect `Vec::from_raw_parts` length, capacity, allocator, or ownership.
- `transmute` across size, alignment, validity, lifetime, or layout assumptions.
- `MaybeUninit` values dropped or read before initialization.
- Self-referential movement and invalid pinning assumptions.
- Pointer arithmetic outside the allocation.
- Double free, allocator mismatch, and ownership confusion at FFI boundaries.
- Data races or incorrect atomic ordering.
- Panics unwinding across an ABI boundary that does not permit it.
- Enum discriminants and `repr` assumptions not guaranteed by the contract.

Prefer dedicated conversion APIs over `transmute`, and slice constructors over manual pointer arithmetic when they express the same invariant.

## 17. FFI and native boundaries

For FFI:

- Use the correct ABI and `repr(C)` or other required representation.
- Honor edition and toolchain requirements for unsafe extern blocks and unsafe symbol-affecting attributes; syntax does not replace the underlying safety proof.
- Treat every signature declaration as unsafe to get wrong.
- Define ownership for allocation, deallocation, strings, buffers, callbacks, and opaque handles.
- Validate nullability, alignment, lengths, enum values, and lifetimes.
- Define thread-affinity and callback-thread behavior.
- Prevent Rust panics from crossing an ABI boundary unless that ABI explicitly supports unwinding.
- Convert foreign errors into Rust errors without losing required codes or leaking private data.
- Keep foreign pointers wrapped in a type that owns the safety contract.
- Match allocators: memory must be freed by the component that owns the matching deallocator unless the API specifies otherwise.
- Test against supported library versions and actual target ABIs.
- Consider dynamic library lifetime; function pointers and borrowed foreign state must not outlive the library or owner.

## 18. Verification for unsafe and security-critical code

Use layered evidence:

- Focused unit and integration tests for invariants and boundary errors.
- Property tests and fuzzing for parsers, pointer metadata, and state transitions.
- Miri for supported tests to detect classes of undefined behavior.
- Loom or another model checker for custom synchronization.
- Sanitizers and platform tooling when configured and supported.
- Static review by someone other than the author for nontrivial unsafe code.
- Real target and ABI tests for FFI.

A passing Miri, fuzzer, sanitizer, or model test does not prove soundness. Maintain the written safety proof.

## Security completion checklist

- Trust boundaries and attacker-controlled costs are explicit.
- Input validation includes resource limits and canonicalization semantics.
- Filesystem, process, network, auth, and tenancy boundaries are safe where applicable.
- Secrets cannot leak through common formatting and diagnostics paths.
- Dependency and lockfile changes have supply-chain review.
- Untrusted Cargo execution is sandboxed.
- Unsafe code is minimal, isolated, documented, and tested.
- FFI ownership, ABI, unwind, and threading contracts are explicit.
- Security exceptions have owners and review conditions.
