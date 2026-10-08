# Codex PR-review evaluations

This opt-in development tool compares real Codex review sessions on disposable, synthetic Git
repositories. It measures whether RepoScout and its bundled skill improve review quality or
reduce the complete interaction's token use. It is separate from the deterministic
[development scenarios](development-scenarios.md) and the unchanged
[historical retrieval pilot](agent-evaluation.md).

No live model run belongs to ordinary CI or a release gate. Offline tests validate the evaluator;
they do not demonstrate better reviews or token savings.

## Comparison conditions

| Arm | Available tools and guidance |
|---|---|
| `baseline` | Codex, Git, `rg`, targeted or justified broader reads, and the fixture's permitted runtimes |
| `reposcout` | The same environment, plus a pinned RepoScout binary and the exact canonical skill bundle |
| `reposcout-cli` | The same environment, plus RepoScout availability without the bundled skill |

The baseline is allowed to solve the task efficiently. The treatment receives no forced command
sequence. Native reads and nonuse of RepoScout remain legitimate observed behavior; tool
availability does not establish tool use or explain an improvement causally.

Both arms receive the same task and business rules. A frozen manifest identifies the fixture,
oracle, public tasks, binary, skill, harness, Codex version, model, reasoning effort, limits and
execution order. Shared task hashes are distinct from arm-specific context hashes: loading a
skill intentionally adds context and its cost must remain in the measurement.

The current study specifies `gpt-6.1-sol` with `max` reasoning. This is an evaluation condition,
not a RepoScout product setting. RepoScout retains its normal configured/default tokenizer and
does not allocate agents or split tasks. Each trial runs one Codex session; only an explicit
follow-up continues that exact session.

## Synthetic review cases

| Case | User need |
|---|---|
| `clean-refactor` | Accept a behavior-preserving change without inventing a defect |
| `refund-boundary` | Find a violated refund deadline and establish the active caller's user impact |
| `import-wiring` | Follow a changed import binding despite an unchanged function body |
| `package-wiring` | Identify the behavior selected by package/configuration wiring |
| `wire-units-noise` | Find a units error among unrelated changes and similar declarations |
| `sparse-evidence` | Obtain missing necessary evidence through targeted follow-up reads |
| `review-followup` | Reassess a later revision, reusing only evidence whose identity still holds |
| `staff-only` | Avoid attributing an independent preview-only change to a production path |

The additional `renewal-holdout` case is excluded from normal smoke and exploratory campaigns.
It is reserved for a separately reported confirmation, rather than tuning the skill on every
available answer.

Business rules and independently executable domain probes define the expected behavior. The
oracle is not derived from RepoScout findings, graph edges, metrics or chosen commands. The
agent sees the application and public review task, but no private expected findings or probe
answers. Base/head commits contain only synthetic source and are signed with a disposable test
identity; they have no remotes or real history. Their commit and tree identities are recorded
separately. Future follow-up source and commits are not inserted into the initial object database.
At initial preparation and every follow-up activation, HEAD, the Git index and the worktree must
describe that same head snapshot. Check actual Git status and both staged and worktree diffs;
matching source bytes alone does not establish a valid Git review environment. A violated fixture
condition excludes the campaign from benefit comparisons even when individual answers are correct.

## Isolation and lifetime

Each assigned run creates a fresh task-owned environment. On Linux, a filesystem namespace
exposes only its synthetic repository, explicit runtimes and the arm's permitted tools. It does
not expose the development checkout, host home, peer runs or private oracles. The evaluated
commands have a restricted permission profile and no network access. A protected controller
authenticates Codex separately; credential files are never agent-readable fixture files.

Before model execution, zero-model probes must establish the configured filesystem and network
boundaries, including a private process namespace. Commands can inspect their own sanitized
process environment; host and controller processes remain inaccessible. Missing support is a
failed prerequisite, not permission to silently run unisolated.
The runner does not attach to or manage an existing Codex daemon. Cleanup targets only resources
created for that trial. The public fixture and copied authentication state are disposable;
private raw traces and sanitized results have separate retention.

Runs are serial and supervised for time, aggregate process memory and available host memory.
Default limits are 180 seconds per episode, 1 GiB process memory and a 12 GiB available-memory
reserve. `prepare --timeout-seconds` can explicitly select a longer episode deadline, up to
600 seconds. Each individual RepoScout child remains limited to 180 seconds. Limits and any
explicitly authorized changes are part of the campaign condition.
Timeouts and interruptions are outcomes, not invisible retries.

Process exit must be established for the whole owned process, including its threads. The
supervisor retains process descriptors and records bounded observation diagnostics privately.
A disappearing process entry may receive a bounded terminal-state check; permission errors,
unconfirmed live identities and persistent observation failures still stop execution. A stale
execution fence also retains its supervisor's PID namespace, so absence in another namespace
cannot clear it. Separate verified recovery evidence may permit retiring a fence; it never
rewrites an aborted outcome or restores missing usage observations.

## Quality and skill behavior

Structured review answers identify the trigger, expected and actual behavior, cause, user impact
and source evidence. Automatic checks validate response structure, exact revision/range/quote
identity and independently established domain witnesses. They do not pretend that matching a
keyword proves a correct causal explanation.
Different witness representations, including a field-focused result instead of a complete response,
require explicit semantic adjudication. A representation mismatch is not itself a demonstrated
domain contradiction. The submitted source identities and verbatim quotes remain strictly checked.

Independent adjudication checks the semantic review against a fixed rubric, before exposing arm
labels or costs. Reviewers and whether they are human or model-assisted are recorded; automated
reviewer judgments are not presented as human validation. This is best-effort blinding: an answer
can reveal the tool it used. Claimed test
execution requires matching controller trace evidence; a plausible answer alone does not prove
that a command ran. Alternative correct evidence chains are allowed. Duplicate findings are not
extra true positives. Missing defects, false alarms, unsupported blockers, wrong-revision claims
and incomplete necessary evidence remain distinct. Clean cases measure false alarms directly.
If a reviewer discovers an additional real defect, record the oracle correction and reassess
both arms consistently rather than selectively accepting one answer.

Skill observations separately record available, observed loaded, invoked and useful behavior.
Only trace-supported facts are asserted; absence of a literal `cat SKILL.md` does not prove a
skill was absent from model context. Help calls, query failures, repeated reads, identity checks
and appropriate native fallbacks help explain the result. An exact prescribed command sequence
is not a quality oracle.

## Usage and reporting

Evaluate correctness and review quality alongside separate input, cache-write, cache-read and
output token counters. Token usage is the efficiency criterion. Wall-clock duration is excluded
from benefit judgments and rankings; server load is uncontrolled. Time limits and controller
timestamps serve process supervision and audit only.

Measure the whole episode, including skill/reference loading, help, retries and follow-up turns.
The native `input_tokens` includes `cached_input_tokens` (cache-read); show that relationship
explicitly. Keep `cache_write_input_tokens` separate when observed and unknown when absent.
Do not assume a disjoint cache-write partition without supporting evidence. Output already
includes observed reasoning; any reasoning breakdown is supplementary.
Input contains cached input exactly once; output contains reasoning exactly once when those
relationships are established by the supported export. Cumulative session counters must not be
added again on resume. The supported binary and observed counter scope are bound to a recorded
calibration; other binaries retain unknown scope until verified. The current exec adapter is a separate, explicitly identified observation
basis; it does not weaken the historical final-provider-call or native-window contracts.

Unknown fields remain unknown. Controller closure, trace integrity and supported usage semantics
are prerequisites for comparison. A successful final answer does not establish complete usage
for failed requests. Tool/source bytes are explanatory measurements, never extra provider tokens.
Subscription charges and money saved are unknown without a supported billing basis. Trial totals
cover the evaluated review sessions; suite development and independent adjudication are separate
experiment operations and are not included in the A/B episode totals.

Every assigned run remains in the report, including not-started, failed, interrupted and
unmeasurable runs. Primary tables show quality and the separate token counters together. A comparison restricted to
quality-passing pairs is explicitly conditional and cannot hide costs or failures elsewhere.
Savings accompanied by poorer reviews do not satisfy RepoScout's objective.
Campaign-level fixture or isolation defects override pair eligibility. Such runs remain visible as
calibration with a machine-readable data-quality exclusion; semantic passes do not restore their
eligibility. A corrected campaign needs fresh preparation, qualification and execution.

Keep raw JSONL, stderr and controller records private. Public exports retain sanitized metrics,
conditions and evidence hashes, not host paths, authentication data or arbitrary raw model/tool
payloads. The 2026-09-12 pilot and its adverse results are not rewritten.

## Campaign stages

1. Offline fixture, grader, accounting, lifecycle and isolation tests, with no model calls.
2. Four smoke cases, two arms: eight episodes to qualify the measurement process. The cases are
   `clean-refactor`, `import-wiring`, `sparse-evidence` and `review-followup`; the two follow-up
   episodes make ten actual Codex invocations. Qualification includes the real second-revision
   activation, exact-session continuation and cumulative-usage boundary.
3. Eight cases, two arms, three repetitions: 48 exploratory runs.
4. Optional CLI-only arm for four preselected cases, three repetitions: twelve additional runs.

The main arms run in balanced seeded order, close in time, using separate fresh environments.
Do not alter prompts, fixtures, skills or limits between repetitions. A change creates a new
campaign version; old failed or adverse runs stay recorded. The smoke gate concerns measurement
and isolation validity, not whether RepoScout happened to win or every review was correct.

These are exploratory samples. Repeated runs on one case are not independent new problem types,
and a small set does not establish a universal percentage improvement. Report individual paired
outcomes and variation. Broader claims need unseen tasks and a larger, separately defined study.

## Running the tools

See the [evaluator commands](../scripts/agent-eval/README.md) for campaign preparation, serial
execution, adjudication and sanitized export. Live runs require explicit authorization and a
working Codex authentication source available only to the controller. Do not put credentials in
the repository or commit private campaign directories.

The same explicit runner can be invoked by a future manual GitHub workflow on a compatible Linux
runner. Such a workflow must preserve the same isolation, model/CLI pins, serial limits and
private-versus-public artifact handling; merely moving a shell command into Actions does not
establish an equivalent experiment.
