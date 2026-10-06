# User efficiency scenario results

The requirements in [user-efficiency-cases.md](user-efficiency-cases.md) were frozen before the
CLI drivers. This test-only extension implements F, G, H, I and K on `test/behavior-scenarios`.
Production source, dependencies, schema/analyzer versions and CI workflows are unchanged.

## Scope and independent review

| Case | Additional user risk covered |
| --- | --- |
| F | A small wire regression remains reviewable amid unrelated PR churn under the same complete-interaction budget. |
| G | Import-only and native ESM configuration changes can alter runtime behavior without editing function bodies. |
| H | Two active applications with identical symbol names require positive evidence of independence or actual coupling. |
| I | A known definition in a naturally large module needs a small binding and actual regression evidence, without dumping the module. |
| K | A follow-up retains justified old evidence and delivers only the necessary delta, including stale-hash handling. |

The scope audit deferred J: its retry investigation substantially overlaps existing investigation,
registration, diagnostics and shadowing scenarios. Not every proposed mutation needs another E2E
run. General filename/creation-order permutations, F serializer relocation, and a separate G
restore-only case remain unimplemented; existing acceptance/navigation coverage is not represented
as proof that these exact new combinations pass. G's main case does include a subsequent live
repair while the driver reads the pinned faulty head.

Literal Python/Node probes establish application behavior independently. Drivers receive task
inputs and public CLI responses only. Personal and independent static reviews checked the packets,
counterfeit controls, provenance and necessary-versus-irrelevant omissions before product runs.
The same evidence predicates reject missing indispensable fragments; variant flags do not invent
the counterfeit's missing evidence. Both tokenizers are explicit representative inputs, without
a redundant full variant-by-encoding matrix.

## Measurements and interpretation

Every public command's complete stdout/stderr is counted before parsing or filtering. Compact
JSON argv costs are separate and also included in total interaction volume. All source bodies,
signatures, patches and repetitions count; shared chunks count once per emission. Initial and
follow-up phases retain separate limits and an episode total. A failed call does not reset costs.

CLI duration ends when its process returns, before transcript writes. Independent fixture probes
and compilation are excluded. `cold` means the fixture's first command; `warm` means subsequent
commands may reuse that isolated cache, not that every requested analysis was a cache hit.
The external guard measures the enclosing serial test process tree. Per-command/per-episode RSS
is unavailable in the ledger and remains `null`; aggregate run peaks must not be presented as
individual measurements. No latency threshold is used as a correctness gate.

## Observations on 2026-10-06

The production implementation is the one at `530c854` (0.4.1 plus the local `read --file` work).
These additions change tests and documentation only. Outcomes below use each case's latest run;
the final wiring driver correction received its own filtered rerun. In total, the latest outcomes
are **21 passed and four failed tests** (25 added tests, 27 CLI episodes). The four failures cover
two distinct unmet needs, not four separate product defects.

| Case | Latest test outcome | Meaning |
| --- | --- | --- |
| F — noisy checkout | 3 passed; six CLI episodes | Both encodings retain necessary request/serializer evidence within the original envelope. Repair and calculator-only counterfeit controls behave correctly. |
| G — wiring | 6 passed | Import-only and native ESM configuration changes, pinned history despite live repair, unused imports and counterfeit checks all meet the original obligations. |
| H — active homonyms | 4 passed | Independent applications remain distinguishable; real rebinding changes the required evidence; a helper-only test cannot prove storefront request coverage. |
| I — sparse invoice | 3 failed; all six CLI episodes executed | Actual module binding and active rounding implementation remain unavailable within the chosen route. The returned data fields and genuine regression evidence now arrive. |
| K — retained context | 4 passed, 1 failed | Helper refresh, stale-hash rejection and both unrelated-change cases pass. Changing the binding repeats an unchanged policy body. |
| Cost ledger | 4 passed | Errors/retries, repeated source, public diff/source shapes, identity scopes and cumulative phase costs remain accounted for. |

I's counterfeit test was actually delivered and rejected as production regression evidence. Its
grouped test remains red because the positive binding-swap episode is incomplete; rejection does
not make that positive task pass. No case uses `should_panic` or an expected-failure wrapper to turn
an unmet user need green.

### Two remaining efficiency gaps

**I: sparse module evidence.** `read --symbol` supplies `net_due`, targeted `find` locates the real
regression, and follow-up reads supply its imports/assertions and the `Invoice` fields. The known
118-line invoice module's `read --file` result is explicitly `budget-omitted`. The small import
binding is therefore unavailable, and the driver cannot establish the actual rounding helper.
A whole-module delivery would also violate the frozen 56 nonblank source-line ceiling. The
original, repaired-helper, switched-alias and unrelated-handler variants retain this gap. This is
an unmet bounded investigation, not a claim that RepoScout miscalculates the business result.

**K: unnecessary repeated source after a binding change.** The public worktree change inventory
identifies the import-only edit; the stale expected hash is correctly rejected. The route then
reads the whole policy module to obtain its new binding, re-emitting seven unchanged policy-body
lines before fetching the new helper. Its follow-up delivers 12 source lines where the independent
delta needs five. Raw response/call limits pass, but the explicit no-repeated-body obligation fails.
The existing `read --line` selector chooses declarations; it is not an arbitrary line excerpt.
This motivates examining a bounded way to retrieve module-level bindings while retaining the
current hash, snapshot, policy and omission guarantees. No new selector is implemented here.

These are failures of exercised public routes against useful frozen needs. They do not prove
that every possible query sequence is impossible. Missing precise binding evidence must not be
filled from a name match or a heuristic import edge.

### Test-driver corrections and scope discipline

Initial runs also exposed weaknesses in the new drivers, which were corrected before classifying
product limitations: H's first response allocation omitted every changed path despite spare total
budget; I's broad name query was crowded by homonyms; G ignored already returned relation/import
paths. Drivers now use those public responses competently under the same frozen total limits.
No fixture was shrunk and no import was moved into a definition to manufacture success.

A final redundancy review removed a separate accounting test whose assertions fit existing checks,
and folded K's oracle sensitivity check into its representative episode. I retains one evidence
artifact per episode rather than duplicating full aggregate reports. The additional retry family
and broad mutation cross-products remain deferred.

### Recorded cost ranges

Ranges cover the executed episodes, including repairs and negative controls. Response tokens
include metadata; source-line counts include signatures and repeated bodies. Identifiers such as
fixture paths and hashes can vary slightly between runs. Timings are observations on this shared
host, not performance guarantees.

| Case | CLI calls | Raw response bytes | Response tokens | Emitted nonblank source lines | Total CLI seconds |
| --- | ---: | ---: | ---: | ---: | ---: |
| F | 5 | 15,448–16,264 | 4,735–4,983 | 30–37 | 0.54–1.23 |
| G | 5–6 | 11,377–14,858 | 3,480–4,588 | 35–48 | 0.66–1.43 |
| H | 5 | 17,300–18,539 | 5,390–5,740 | 50–63 | 0.48–1.37 |
| I | 4 | 4,687–4,691 | 1,402–1,441 | 19 | 0.39–1.03 |
| K | 5–8 | 6,331–8,529 | 1,932–2,591 | 17–29 | 0.75–1.81 |

I's small response is insufficient, so its size is not a success metric. Full command-argument
and combined-interaction costs are retained alongside these response costs in each ledger.

### Validation and resource limits

- Targeted release-profile Clippy with warnings denied passed; formatting and shell syntax passed.
- The 18 existing acceptance tests passed in 9.33 seconds and seven existing journeys in 17.13
  seconds, covering the shared helper changes.
- The full new efficiency group took 36.23 seconds excluding compilation before the final G-only
  follow-up correction. Its monitored build plus run peaked at 644 MiB; existing-group runs peaked
  at 188 MiB and 238 MiB. At least 21,238 MiB system memory remained available in that run.
- Builds and CLI validations were run serially under 1 GiB/180-second guards, with a 12 GiB host RAM
  reserve. The earlier one-time compiler exception was not reused.
- Two direct test-binary attempts omitted Cargo's serialized environment and were stopped by the
  ownership-scoped guard after 0.3 seconds; both groups were rerun correctly through Cargo. These
  aborted attempts are not product failures. The sandbox also blocks the existing child-timeout
  signal pipe, so actual process scenarios ran outside it with the same guards.
- The unchanged broader Rust/frontend suites and the original 20 focused development scenarios
  were not rerun for these test-only additions. No automatic CI jobs were added.

The final six-case wiring rerun passed in 8.31 seconds (46.6 seconds with recompilation, 646 MiB
process-tree peak). The ordinary development target reports all 70 tests ignored. The final
`cargo build --release --offline --locked -j1` passed and refreshed the development binary;
`reposcoutdev` still resolves to this worktree's release binary. A bounded `read --file` sanity
check against the existing sample fixture returned complete source.

These measurements do not establish lower model bills, better real-world review accuracy or
superiority to a competent `git diff`/`rg`/targeted-read workflow.

## Reproduce

```sh
./scripts/test-scenarios.sh --keep-failed efficiency
./scripts/test-scenarios.sh --keep-failed efficiency::invoice
./scripts/test-scenarios.sh --keep-failed efficiency::reuse
```

Run one filtered family at a time under the repository's resource limits. Python 3 is required;
G's ESM case additionally requires Node.js, with no package installation. `--keep-failed` preserves
synthetic repositories, independent truth, frozen packets, full command transcripts and ledgers
outside the scanned repository. Successful fixtures clean up; `--nocapture` still prints their
cost summaries. The full `efficiency` filter currently exits nonzero for the four unmet cases.
All scenarios remain ignored in ordinary tests and automatic CI. The runner can
be used by a future manually dispatched workflow; no workflow was added here.
