# End-to-end CLI journeys

The `journeys` group in the opt-in development scenarios plays through complete RepoScout tasks
using real CLI processes. The user steps are ordinary deterministic test code. There is no model,
agent harness, browser, network service, or automatic review decision involved.

## Design contract

Each journey has three distinct parts:

1. **World and task:** an authored synthetic repository, optional Git revisions or diagnostic log,
   and a realistic starting task such as a search phrase or a base/head comparison.
2. **Driver:** receives only the task inputs and a CLI execution handle. Commands consume actual
   preceding responses to discover paths, qualified symbols, source hashes, revisions and gaps.
   Expected fixture paths and source text are not available to this part of the test.
3. **Oracle:** independently checks the final evidence against the authored world: exact source,
   relevant callers, old/new identities, negative controls, statuses and completeness. It never
   supplies a missing target to the driver.

The public seam is command arguments, exit status and process output. Source reads use returned
identities and expected hashes wherever discovery supplies them. Health finding locations do not
carry hashes: that journey holds the fixture unchanged until the first read establishes its content
identity, and makes no atomic-snapshot claim. A pinned review uses its returned Git trees; the
worktree-only `consumers` query is reserved for worktree journeys. Existing default or explicitly
configured tokenization applies throughout.

Follow-up policy is finite and visible in the test. A driver can expand a budget or graph depth
when a response explicitly reports omissions, up to the task's stated ceiling. At that ceiling,
remaining gaps are a result to inspect, not a successful claim of complete evidence. Empty output
must not satisfy the oracle vacuously. Authored fixture repairs and live-file edits are explicit
test actions outside the driver.

## Task families

| Journey | Starting knowledge | Evidence-driven route | Independent outcome |
| --- | --- | --- | --- |
| Guard-removal PR | Repository, base/head, budgets | Review discovery → changed declarations and concrete callers → pinned source reads | Both function versions and real unchanged callers; no homonym or dirty-worktree contamination |
| Module-migration PR | Repository, base/head, budgets | Revision-local changes and aliases → per-side targets → pinned reads | Added/deleted sides, unchanged rename identity and old/new bindings remain distinguishable |
| Budget-limited PR | Repository, base/head, bounded budget choices | Review omissions → bounded re-query → returned definition/source targets | Small relevant changes remain reachable; budget exhaustion remains explicit |
| Symptom investigation | Search phrase and budgets | Find → checked read → stale-identity recovery → consumers → source plan | Exact relevant call chain and whole source; shadowed and unrelated names excluded |
| Compiler-diagnostic triage | Scope, diagnostic log and budgets | Diagnostic context → resolved positions → definition plan → source reads | Valid diagnostic seeds survive tight context limits; uninventoried paths stay explicit gaps |
| Regression investigation | Repository, baseline, threshold | Baseline changes → new/worsened locations → definition reads → repair and gate recheck | Existing debt is not a new regression; an incomplete comparison is not clean |
| Production clone cleanup | Repository and cleanup policy | Ranked production clone locations → source reads → authored extraction and recheck | Production duplication is resolved while independent test-only duplication stays visible |

These extend the existing focused development scenarios. They establish that a scripted public
CLI workflow can reach the evidence needed for the authored task; they do not measure an actual
agent's review quality or prove runtime impact completeness.

The diagnostic journey starts with a scoped inventory. A sibling package and an ambiguous basename
are therefore unresolved inputs, not evidence that the resolver knows their actual definitions.
The driver keeps those gaps and follows only resolved positions; it does not broaden the scan or
guess a source path from the fixture.

## Run locally

```sh
./scripts/test-scenarios.sh --list journeys
./scripts/test-scenarios.sh journeys
./scripts/test-scenarios.sh journeys::review
./scripts/test-scenarios.sh journeys::navigation
./scripts/test-scenarios.sh journeys::health
./scripts/test-scenarios.sh --keep-failed journeys
```

Every journey is ignored by ordinary `cargo test`, including current CI. The existing runner
refreshes the release binary and retains the serialized harness and shared two-worker CLI limit.
Use a family or test-name filter for narrow investigations and resource-bounded validation.

On 2026-10-06, all seven journeys passed on the development Linux host in 69 seconds after
compilation, with a measured 238 MiB peak for the monitored process tree. These are local
observations, not timing guarantees or CI estimates. The guard-removal journey also ran against
the previously verified official v0.4.0 binary: the same test failed because its head-side changed
declaration was missing, while the current binary passed. The suite itself needs no old binary
or network download.

The original 20 scenarios were also rechecked in four serialized groups. The normal target run
confirmed all 27 tests remain ignored. Formatting, focused Clippy for this integration target and
the release build passed. Validation was scoped to this test-only change; the full production Rust
suite, all-target Clippy and macOS execution were not rerun.

Each named step records its arguments, exit status, stdout and stderr under the fixture's private
state directory outside the scanned repository. JSON remains available even when a later assertion
fails. Normal fixture cleanup removes the transcript; `--keep-failed` retains it together with the
failed synthetic repository and prints its location. These records contain synthetic source and
diagnostics, not a scan of the developer's repository.

## Future manual CI use

The same `./scripts/test-scenarios.sh journeys` command is suitable for a future manually triggered
GitHub Actions job on a supported Unix Rust runner. The tests need no interactive credentials,
model access, daemon or external fixture downloads. A future job can use `--keep-failed` and retain
the printed failure directories for diagnosis. No workflow or default CI execution is added here.
