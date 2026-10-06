# User efficiency cases: sufficient evidence within an interaction budget

Requirements frozen **2026-10-06, before implementation and product runs**. They extend the
[frozen A–E user needs](user-acceptance-cases.md); they do not change their fixtures, criteria or
[observed results](user-acceptance-results.md). Two fresh domain designers used public contracts
and the earlier requirements, without inspecting implementation, test code or measured outputs.
Implementation scope and subsequent observations live in [efficiency results](user-efficiency-results.md);
the requirements and budgets below remain unchanged.

The goal is enough trustworthy context to make the user's stated decision, with little irrelevant
or repeated material. A complete repository dump and an empty compact answer must both fail.
RepoScout supplies evidence; these tests do not require it to diagnose the application, choose a
model, allocate agents or run the application itself.

## The public test boundary

Use the existing opt-in CLI scenario infrastructure with three separate responsibilities:

1. The fixture author defines the application, literal business examples, revisions, independent
   one-shot probes and a manually assembled sufficient evidence packet.
2. The driver receives only the stated user task, repository/revisions, budgets and public CLI
   handle. It discovers follow-up targets from responses. No fixture reads, oracle paths, private
   parser calls or probe results may fill a gap in delivered evidence.
3. The evaluator checks the complete transcript against the independent obligations and budgets.
   It may not select a convenient subset of an oversized answer to claim efficiency.

The hand-authored packet establishes feasible evidence and explains each fragment's purpose; it
is never returned to the driver. Freeze it, executable examples, distractors and numeric limits
before observing RepoScout output. If the packet cannot fit, correct the fixture/design before
the first product run and record why. Later failures must not move imports into functions, shrink
natural modules, relax budgets or turn unmet positive tasks into expected successes.
For the packet-relative limits below, `t(P)` means the sum of token counts of the frozen necessary
source fragments, each counted as delivered. It excludes explanatory prose, oracle answers and
identity metadata; the fixed extra allowance covers provenance and navigation. The packet's
escaped-source size counts the bytes of each source string serialized as JSON, including quotes.

Allow any public CLI route that meets the obligations. Exact attributable source can establish
bindings; an equally precise public static-binding record can substitute where it identifies the
actual site, target and content/revision identities. A heuristic import edge or name match cannot.
Behavioral expressions and real test inputs/assertions must still be delivered. An explicitly
identified excerpt can establish a binding without pretending to be a complete function/file;
this design neither invents an existing excerpt command nor permits silently clipped source.

Every delivered fragment must belong to the requested snapshot/content identity. In worktree
cases, use known expected hashes and keep fixture mutations between declared phases; do not claim
atomic consistency across live files. Pinned cases retain both revision identities. A later live
repair must never replace the requested faulty historical head.

## Measure the whole interaction

Use separate ledgers, never a single ambiguous “tokens saved” score:

- **Obligations:** every required fact, binding and applicable regression check is accounted for.
  Honest missing evidence is a failed positive task, distinct from incorrect evidence.
- **All responses:** sum raw stdout and stderr for every public invocation before driver parsing
  or filtering, including metadata, newlines, errors, retries, omissions and repeated bodies.
  No post-hoc projection makes a large response free. Also count command arguments as compact
  JSON argv bytes/tokens and report their sum with responses as total CLI interaction volume;
  the numeric response ceilings below apply to stdout/stderr, not to an invented model bill.
- **Delivered source:** count every emitted source, patch and signature occurrence, including
  irrelevant and repeated text. Shared chunks count once when emitted once; references still
  consume response budget. Track nonblank lines, tokens and distinct repository-relative paths.
  Before/after bodies both count even when their path is the same.
- **Work:** record CLI calls, wall time per command/episode, process-tree peak RSS and cold/warm
  state. Compile time and independent fixture-probe time are separate from RepoScout execution.

Token counting uses the effective existing default or the explicit user/configured encoding.
The test never chooses an encoding by model or switches to whichever makes a case pass. Exercise
the two supported encodings as explicit input variants. Count each response separately before
summing; source tokens are a separate diagnostic and are not a replacement for response tokens.

The common PR envelope is **7,500 response tokens, 24 KiB raw responses and six CLI calls** per
episode; both size limits apply. This is an acceptance target chosen before measurement: up to
2,000 tokens for necessary source, 3,000 for identity/binding/coverage facts and 2,500 for bounded
discovery/follow-ups/errors. Those shares explain the ceiling, not mandatory output allocations.
Per-case source limits below are tighter. Any smaller navigation/follow-up envelope is explicit.

Missing coverage cannot be treated as zero impact to meet a size limit. A useful positive result
must fit and satisfy the obligations. Negative controls test rejection of counterfeit evidence;
they are reported separately from positive task completion. Removing any indispensable obligation
from an otherwise sufficient packet must make its evaluator reject that packet.

These are deterministic context-efficiency tests, not model billing measurements. They do not
count hidden reasoning, conversation replay or provider caching and cannot establish monetary
savings. Source retention avoids another tool delivery; it does not make later model inputs free.
No shared-host millisecond threshold is a correctness gate. Keep ordinary resource/time guards,
report timings, and measure comparable repeated cold/warm runs separately before claiming speedups.

## F. A small wire regression among unrelated PR changes

**Task:** Given only repository and base/head: review compatibility of `POST /checkout`.
`amount_cents` must remain an integer number of cents and currency must remain `EUR`. This is a
scoped compatibility task, not a request to approve every change in the PR.

**Truth:** Requests for two items at 1,250 cents and one at 99 cents produce
`{"amount_cents":2500,"currency":"EUR"}` and `{"amount_cents":99,"currency":"EUR"}` at base.
Head adds integer division by 100 in the serializer, producing amounts 25 and 0. The calculator
remains correct. A real dispatcher-level test retains the literal 2,500/99 expectations.

**Required packet:** actual endpoint registration/dispatch; handler bindings connecting the
calculator and serializer; calculator's units; old/new serializer; the request inputs, assertions
and binding to the dispatcher. Each is needed to connect the changed expression to the real wire
contract. A correct calculator test alone is insufficient.

**Efficiency challenge:** The noisy variant adds independently behavior-preserving formatting
changes in unrelated telemetry/report code and generated snapshots. Compact changed-path/coverage
accounting is permitted; those bodies and patches are not needed. The quiet and noisy episodes
have the same evidence/source limits and common response ceiling. Noise may add at most **4 KiB**
to total raw responses, allowing bounded path/coverage records without dumping the mechanical diff.

**Bounds:** At most 32 fixture files / 128 KiB; five delivered source paths, 110 nonblank source
lines, 2,000 source tokens; common PR envelope. No source is padded to a parser/detector threshold.

**Sensitivity:** Repair the serializer; move the endpoint to a different serializer while retaining
the old decoy; rename internal paths and reverse creation order; replace the same-path request
test with calculator-only assertions. Required provenance moves with the real route, and the
counterfeit test must not satisfy request-contract evidence.

## G. Changed wiring with unchanged function bodies

**Task:** Given repository and base/head, review a shipping wiring cleanup. The active quote
endpoint must still charge 499 cents domestically and 999 internationally.

**Truth:** Retail returns `[499, 999]`; supplier returns `[299, 799]` for the two regions. Base binds
the handler's local policy name to retail. Head changes only the module-level import to supplier.
Route, handler and both policy bodies remain byte-for-byte unchanged. Real endpoint responses
change accordingly; the request-level tests retain `[499, 999]`.

**Required packet:** old/new active binding, handler call using it, route/dispatch, both selected
policies and actual request/assertion bindings. Unchanged bodies can be affected evidence without
being falsely described as edited declarations. A missing changed-function mapping is not evidence
of unchanged behavior.

**Bounds:** At most 16 fixture files / 48 KiB; six delivered source paths, 100 nonblank source lines,
1,800 source tokens; common PR envelope. Unused homonym policies and unrelated utility bodies do
not belong in the packet.

**Sensitivity:** Restore retail; add an unused supplier import without changing the active call;
rename policy paths; repair only the live worktree after committing faulty head; retarget a
same-path test directly to retail so it no longer exercises the endpoint.

**Configuration variant:** In a separately frozen plain JavaScript ESM fixture, a checked-in
nearest-package `imports` mapping for `#tariff` changes from retail to supplier. Source imports
and function bodies stay unchanged. A one-shot Node probe verifies actual package resolution;
old/new package configuration must replace the changed-import obligation. Interpreter absence is
an environment failure, not an acceptance result. No package install, server or network is needed.

## H. Same symbol names in independently wired applications

**Task:** Review whether a discount change affects the storefront API or only staff preview.
Storefront accepts exactly `WELCOME` on subtotals of at least 2,000 cents, discounting 500 cents.
The user supplies repository/base/head and this rule, not internal symbol paths.

**Truth:** Two packages have same-basename rules modules and same-named functions, each bound by
its own API's static relative import. Staff deliberately has no subtotal threshold; its head also
accepts codes beginning with `WELCOME`. For `(WELCOME,1999)`, `(WELCOME,2000)`, `(WELCOME-X,2000)`:

| Active application | Base discounts | Head discounts |
| --- | --- | --- |
| Storefront | `[0, 500, 0]` | `[0, 500, 0]` |
| Staff preview | `[500, 500, 0]` | `[500, 500, 500]` |

**Required packet:** staff policy before/after, storefront policy, both active dispatch/import/call
chains and genuine request checks with package identity. This establishes independence from
positive binding evidence, not an empty consumer list. A demo package exporting the same names,
application-wide settings and unrelated handlers are distractions.

**Bounds:** At most 20 fixture files / 64 KiB; six delivered source paths, 110 nonblank source lines,
2,000 source tokens; common PR envelope. Route registrations may live naturally in each API module.

**Sensitivity:** Rebind the real storefront import to staff at head: storefront then returns
`[500, 500, 500]` and is genuinely affected. Rebinding only its test is a counterfeit control, not
proof that the storefront changed. Rename package roots, change creation order and add homonyms;
none may change which evidence establishes actual package identity.

## I. A known definition needs a small binding, not its entire large module

**Task:** The user knows `src/invoices.py::net_due`: why does a 1,005-cent invoice with a 1,000
basis-point discount cost 905 instead of 904 cents? Supply the necessary change site and a genuine
regression check. The business rule rounds discount amounts to the nearest cent, halves upward.

**Truth:** `net_due` receives an `Invoice`, calls an imported `discount_cent` alias and subtracts
that discount. The active helper uses Python's `round`; the 100.5-cent discount rounds to 100.
The independently authored integer repair is `(amount * bps + 5000) // 10000` for nonnegative
integer amounts and integer discounts from 0 through 10,000 basis points.

| Amount / basis points | Faulty payable | Repaired payable |
| --- | ---: | ---: |
| 1000 / 1000 | 900 | 900 |
| 1005 / 1000 | 905 | 904 |
| 1005 / 0 | 1005 | 1005 |
| 1005 / 10000 | 0 | 0 |

**Required packet:** actual import binding, `Invoice` fields establishing units, `net_due`, active
helper, and a real test bound to this production path with literal `904`. The target alone cannot
identify the alias implementation; an unconnected helper cannot establish production use.

The invoice module also contains substantial, naturally written pagination, export, preview and
reporting handlers. Requesting the entire module violates the source ceiling. Tiny files and
imports moved inside functions would remove the intended challenge and are not acceptable fixtures.

**Budget derivation:** Hand-author at most 42 necessary nonblank lines: binding 2, data declaration
8, target 8, helper 8, regression evidence 16. Their JSON-escaped source is at most 3 KiB, across
four paths. Evaluate `t(P)` under the run's effective encoding. The episode permits four CLI calls,
four source paths, **56 source-line occurrences**,
**9 KiB responses**, and **t(P) + 2,400 response tokens**. The byte reserve is 6 KiB for discovery,
identities and coverage; the extra 14 source lines allow modest surrounding context. Fixture
ceiling: 20 files / 48 KiB. Freeze exact source fragments and spans before computing `t(P)`.

**Sensitivity:** Repair only the actual helper; switch the alias while retaining the old helper;
retarget the same-named test to an unrelated correct implementation; add/rename unrelated handlers
without increasing the envelope. Each positive route must supply the current relevant binding and
implementation. The finite examples do not certify a general-purpose rounding library.

## J. Follow the active retry failure instead of a same-name distraction

**Task:** Production job `payments.retry`, with `retry_after_ms=1250`, misses its deadline. With
virtual current time 100 seconds and deadline 102, it should execute at 101.25. A short trace gives
real coordinator/executor locations as user input. Show the registered path, conversion, cap,
comparison and a relevant regression check.

**Truth:** The registered coordinator calls a statically imported converter and then an executor.
The converter incorrectly returns milliseconds unchanged; scheduling caps the delay at 30 seconds.
Repair divides by 1,000 before the cap. All fixture execution is synchronous with literal virtual
time: no clock sleeps, watcher or actual job service.

| Delay in ms | Faulty time / result | Repaired time / result |
| --- | --- | --- |
| 0 | 100 / sent | 100 / sent |
| 1250 | 130 / deadline missed | 101.25 / sent |
| 45000 | 130 / deadline missed | 130 / deadline missed |

**Required packet:** actual job registration/import, coordinator-to-converter/executor bindings,
conversion, cap/deadline expressions, and a test dispatching the registered job with real inputs
and assertions of both scheduled time and status. The 45,000-ms control must require time 130:
removing the cap still misses the deadline, so a status-only assertion would accept that wrong
repair. The independent probe also checks both values.

Correct demo/archive converters, a highly connected unrelated retry utility, converter-only tests
and prose repeating the error text are distractions. Ranking highly or sharing a name is not
evidence of participation in the active failure.

**Budget derivation:** Necessary packet at most 84 nonblank lines / 4 KiB escaped source / five
paths. Episode permits six calls, six source paths, **112 source-line occurrences**, **14 KiB
responses**, and **t(P) + 3,200 response tokens**: 28 extra lines and 10 KiB accommodate bounded
navigation/provenance. Fixture ceiling: 20 files / 48 KiB.

**Sensitivity:** Repair the actual converter while leaving a faulty demo; switch the registration
to another coordinator; retarget the test to a demo job; add a local/parameter binding shadowing
the imported name. The last case requires the actual supplied binding evidence, not an invented
imported-call edge. Changes solely in archive code/prose must not change the necessary packet.
This does not require runtime-complete static analysis or executing the job inside RepoScout.

## K. Continue an investigation without delivering unchanged source again

**Initial task:** At HEAD, why is storage status for 895 of 1,000 bytes still `clear`? Supply the
active entrypoint, percentage calculation and status boundaries.

**Follow-up:** The user changed the percentage calculation and asks for an updated explanation for
the worktree while retaining already established unchanged context. The driver may keep its own
prior responses and identities; RepoScout does not gain session state.

**Truth:** The entrypoint calls `quota_status`, which imports `rounded_usage`. Status is `blocked`
at at least 100%, `warning` at at least 90%, otherwise `clear`. HEAD rounds down; the main follow-up
changes only the helper evidence file to round up. Use integer arithmetic and positive capacity.

| Used / capacity | HEAD | Follow-up |
| --- | --- | --- |
| 0 / 1000 | clear | clear |
| 890 / 1000 | clear | clear |
| 895 / 1000 | clear | warning |
| 900 / 1000 | warning | warning |
| 1000 / 1000 | blocked | blocked |

**Required packet:** Initially, entrypoint/bindings, policy/boundaries and helper. The caller's ledger
retains exact captured source, hashes, revision identity, spans and fulfilled obligations. After
the controlled between-phase mutation, public change/identity evidence must justify which old
fragments remain valid; a new helper hash alone cannot prove other files unchanged. Fetch the
current helper and reject stale source. No edits occur during either phase.

**Budget derivation:** Initial packet `P1`: at most 48 lines / 2 KiB escaped source / three files.
Delta packet `P2`: the changed helper, at most 12 lines / 1 KiB escaped source / one file.

| Phase | Calls | Delivered source ceiling | Raw responses | Response tokens |
| --- | ---: | --- | ---: | ---: |
| Initial | 4 | 3 paths, 64 lines | 8 KiB | t(P1) + 2,000 |
| Helper-only follow-up | 3 | 1 changed path, 16 lines | 6 KiB | t(P2) + 1,600 |
| Complete main episode | 7 | Sum of both phases | 14 KiB | Sum of both phase caps |

Every initial response remains part of the episode total. Repeating complete unchanged entrypoint
or policy bodies fails the follow-up requirement. Fixture ceiling: 20 files / 48 KiB.

**Sensitivity and separate delta packets:**

- A stale-hash attempt must deliver no stale source. Its response and the refresh both consume
  the same follow-up budget; no reset after a failed attempt.
- Supplying the old floor body or attaching a new identity to old bytes must fail provenance.
- An unrelated-file edit permits zero new source-body lines; bounded complete change/identity
  evidence suffices. Freeze `P2` as an empty necessary-source packet, so `t(P2)=0`: three calls,
  6 KiB responses and 1,600 response tokens remain the follow-up ceiling. Signatures still count
  in the 16-line ledger and all metadata counts.
- An unrelated declaration edited in the same helper file requires current identity plus complete
  disjoint-change evidence to retain the old relevant fragment. Do not relabel its bytes with a
  new identity without that proof or automatically reread the entire file. This variant also has
  empty `P2`, zero new source-body lines and the same three-call / 6-KiB / 1,600-token ceiling.
- Changing the active import to a new helper expands the legitimate delta. Freeze that separate
  packet first: at most two paths, 24 necessary lines / 2 KiB escaped source; allow four follow-up
  calls, 32 emitted lines, 8 KiB responses and t(P2) + 2,400 tokens. The complete variant has eight
  calls / 16 KiB responses. This is an independently larger information need, not a raised limit
  to conceal failure of the original helper-only case.

## Implementation order and reporting

Start with **I (small binding in a large file), G (wiring-only PR), then K (incremental reuse)**.
They directly challenge sufficient context, coarse whole-file expansion and repeated delivery.
Continue with F, H and J for noise resistance, application scope and causal navigation.

Implement one vertical case at a time in the existing ignored `development_scenarios` acceptance
group after freezing its literal packet and independent probe. Add the repair, counterfeit and
noise/identity variants for that case before moving on. A separate reviewer checks the oracle,
fixture naturalness and driver isolation before inspecting product outcomes. Keep all primary
cases opt-in even if they run quickly; promoting any small stable subset to normal CI is separate.

Reports list each missing obligation, wrong identity, source/response excess and call excess
separately. Record quiet/noisy and initial/follow-up totals, plus per-case runtime and RSS. Failure
artifacts retain the fixture, independent truth, exact CLI arguments, complete responses and cost
ledger. Existing manual runner conventions remain suitable for a future workflow-dispatch job;
no workflow, dependency, server or model harness is introduced by this design.

Do not report the six designs alone as new bugs or as passing tests. Correctness/efficiency outcomes,
any fixes and measured gains belong in a separate results document after execution. Even passing
all cases establishes only these bounded synthetic information needs, not general review quality
or superiority to a competent `git diff`/`rg`/targeted-read workflow.
