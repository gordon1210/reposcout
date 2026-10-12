# Large review evaluations

This local development study extends the [existing isolated Codex evaluator](codex-review-evaluation.md)
with substantial synthetic applications. The harness is implemented; fixture qualification and
model execution are separate gates. A prepared plan or passing offline test is not an observed
product improvement. Historical campaigns and exports remain unchanged.

This document describes reproducible evaluation methods, not observed campaign outcomes.
Raw results and controller evidence belong outside Git. Offline tests are not a product benchmark.

## Fixed question and scope

The priority is fewer model tokens at equal or better review quality. A modest token increase is
acceptable only alongside a clear material quality improvement. Report actual quality failures and
each token field separately; do not invent a weighted score, infer equivalence from two repeats,
or use elapsed time as efficacy evidence.

There are exactly 36 assigned single-invocation reviews, all using `gpt-6.1-sol` with `max` effort:

| Stage | Cases | Conditions | Repeats | Assignments |
|---|---:|---|---:|---:|
| Development before | 3 | Native tools, original RepoScout and canonical skill | 2 | 12 |
| Development after | Same 3 | Native tools, improved RepoScout and canonical skill | 2 | 12 |
| Held out | 2 | Native tools, original treatment, improved treatment | 2 | 12 |

Development covers two regressions and a clean change in a Python order/fulfillment application.
The private heldout application uses Node ESM and a distinct document-publication domain. Its
source, tasks and independent oracles must be sealed before the first development model output.
Its fixture author and independent reviewer retain those details away from the product-tuning owner until
the single improvement is frozen. Neither adverse results nor easy tasks authorize replacements
or favorable extra repetitions.

Applications need substantive connected production behavior, competing legitimate workflows and
ordinary useful tests. File count, documentation volume, repetition and pass-through wrappers do
not establish a large scenario. Record production/test/documentation size separately. Private
probes derive expected business outcomes independently of RepoScout; ordinary visible tests should
not simply disclose the seeded defect and its full witness. Efficient native review remains valid.
Quality admits alternative correct witnesses and source chains, including honest complete static
reasoning. A shorter decisive proof earns full credit: there is no minimum number of files, reads,
commands or hops, and no need to demonstrate consumers unrelated to the claimed effect. Clean
equivalence can be established at a shared representation contract. Any supplied source quotation
must retain exact source/revision identity; a clean answer may omit final quotations when its visible
contract permits that, while still requiring semantic review. Independently verify unexpected genuine
defects instead of automatically
classifying findings absent from the seeded oracle as false positives.

The qualified applications have the following base inventories. These are bounded synthetic
in-memory applications, not evidence from industrial repositories or monorepos.

| Application | Production | Tests/support | Documentation | Whole base |
|---|---:|---:|---:|---:|
| Meridian development | 120 files, 228,890 bytes, 4,587 lines | 45 files, 72,505 bytes | 8 files, 20,152 bytes | 173 files, 321,547 bytes |
| Private publication holdout | 82 files, 211,220 bytes, 4,153 lines | 70 files, 104,494 bytes | 15 files, 64,016 bytes | 168 files, 379,963 bytes, including one 233-byte manifest |

These counts describe one application baseline, excluding duplicated case copies, harness-injected
guidance, private probes and Git objects. Development heads contain 173–174 files and 321,571–321,667
bytes; heldout heads contain 169 files and 381,691–381,700 bytes. Source bytes are inventory facts,
not model token estimates. The catalogs advertise `python3 -B -m unittest discover -s tests` for
development and `node tests/run.mjs` for heldout public checks. Head tests are available to all
conditions; honest tests are never removed merely to conceal a seeded defect.

Meridian connects purchasing, quarantine/receiving, stock reservations, recurring checkout,
fulfillment, carrier operations, accounting and customer views through shared state and authenticated
request paths. One cancellation review is intentionally easier. The event-routing review concerns
a shared identity contract across scoped reconciliation and live ingestion; the third is a clean
extraction. The [local development bundle](../scripts/agent-eval/fixtures/large-review/development/)
retains complete sources, independent oracles, probes, counterfactuals and qualification recipes.
Heldout case identities, task-specific mechanisms and answers are intentionally omitted here.

The durable local reproduction paths are:

| Artifact | Path | Integration state |
|---|---|---|
| Development bundle and recipes | `scripts/agent-eval/fixtures/large-review/development/` | Integrated unchanged |
| Exact sealed heldout bundle | `scripts/agent-eval/fixtures/large-review/holdout/` | Integrated unchanged after the development-after model stage |
| Heldout source, recipe and oracle archive | `scripts/agent-eval/fixtures/large-review/holdout-reproduction.tar.gz` | Integrated with the original archive digest |

The heldout archive is a controller artifact containing the exact bundle, construction inputs,
original recipes, independent design/audit records, original private qualification observations,
an inventory of file digests and `REPRODUCE.md`. It excludes keys, caches, model sessions and bulky
materialized qualification workspaces. Its SHA-256 is
`628833ce0d4e996c49603ca847ca1e24b75f10f3154bee551c0f3b9e1f96c97a` (355,295 bytes).
The sealed heldout bundle path/digest-map SHA-256 is
`978aa1142e57cf637f5f29ca122d0f6b8dbd4c375c573b7118b495f62d209497`.
Extraction into a fresh private directory relocates the bundle for `--case-bundle`; absolute paths
in historical receipts remain provenance. Run construction recipes only in fresh disposable
copies, and retain new qualification receipts separately. Never expose the archive or an entire
bundle, which contains private oracles and sibling heads, to an evaluated agent.

## Reused preparation and execution

`review_campaign.py prepare` accepts the bounded presets `large-development`, `large-holdout`,
`large-holdout-original` and `large-followup`. They require an explicit `--case-bundle` instead of modifying the old
micro-study catalog. The external `catalog.json` contains per-case `check_command` arguments and
explicit `source_limits` no larger than 256 files and 2 MiB per snapshot, including the base.
An optional relative `base_directory` shares an application baseline across independent heads.
Application `README.md` remains intact; injected `REVIEW_CHECKS.md` supplies the advertised command.
The bundle hash includes the catalog, application sources, private oracles and probes.

The separate `large-followup` preset fixes three PR tasks, two repeats and native/skill
pairs: twelve single-invocation assignments. It preserves the original 36-review study and the
unexecuted two-case proposal. Its reused opaque IDs do not identify the old tasks: comparisons must
remain within the new campaign and match source, revision and prompt identities. The application
is already exposed; two tasks are coupled bug/clean variants and the third adds a context question.
A different PR set cannot identify a V1-to-V2 treatment effect.

Use one private disposable `--fixture-signer` record for all matched campaign versions, so identical
source comparisons also retain identical signed commits, tasks and oracle identities. The original
binary and exact canonical skill are archived before tuning. The original heldout preset requires
`--original-campaign` pointing to the original development plan and checks both archived hashes
against that prior canonical condition. It does not accept an arbitrary historical skill override.

Pin binaries, runtimes, skills and calibration in the private plan.
Keep those installation fingerprints outside the public repository.

Preparation is phase-specific. First prepare `development-original` while the repository's
canonical skill matches the original condition. Omit `--fixture-signer` on that first preparation
to create its disposable signer, then reuse its private `signer.json` for the remaining campaigns.
Only after development diagnosis and candidate freeze should the canonical skill match the
candidate for `development-improved` and `holdout-improved`. Preserve the original campaign and
archived treatment for `holdout-original`; do not edit existing plan files to substitute pins.

| Campaign directory | `--stage` | Bundle | Treatment | Assigned episodes |
|---|---|---|---|---:|
| `development-original` | `large-development` | Development | Original canonical | 12 |
| `development-improved` | `large-development` | Same development | Frozen candidate canonical | 12 |
| `holdout-improved` | `large-holdout` | Heldout | Frozen candidate canonical | 8, including the shared native arm |
| `holdout-original` | `large-holdout-original` | Same heldout | Archived original | 4, original treatment only |

For each phase, choose a new private campaign destination and the matching archived paths, then
use the existing command below. The uppercase variables denote operator-supplied absolute paths;
`CAMPAIGN_STAGE` is the corresponding table value. For the first original development preparation,
omit the signer argument. For original heldout preparation, additionally pass
`--original-campaign "$STUDY_ROOT/development-original"`.

```sh
python3 scripts/agent-eval/review_campaign.py prepare "$CAMPAIGN_DIR" \
  --stage "$CAMPAIGN_STAGE" --seed 20261008 \
  --codex-binary "$PINNED_CODEX" --codex-version 'codex-cli VERSION' \
  --reposcout-binary "$PINNED_REPOSCOUT" --skill-dir "$PINNED_SKILL" \
  --case-bundle "$CASE_BUNDLE" \
  --fixture-signer "$STUDY_ROOT/development-original/signer.json" \
  --timeout-seconds 600
```

Reproducing the original canonical preparation after the candidate exists requires an appropriate
separate reproduction checkout with that original skill and the qualified harness. An archived
skill directory alone does not bypass the canonical-skill check. Do not change the active study's
worktree, frozen skill, source bundles or runtime pins while its assignments are running.

Before models, run the offline evaluator tests, independent base/head/counterfactual probes, public
checks and the actual filesystem/network/process/runtime preflight. An unchanged calibrated
single-review protocol needs no additional result-selected live smoke. Keep the same native binary,
runtime inventory, answer schema and resource limits across conditions. Execute serially under the
existing supervisor: 600 seconds per episode, 1 GiB aggregate RSS, a 12 GiB host reserve, one
RepoScout child with its 180-second limit and two configured workers. These are safety bounds.

Qualification runs separately from assigned reviews and cannot establish a product benefit. The
development bundle's `qualify.py` and heldout archive's `qualify.py` retain their existing independent
base/head and counterfactual recipes. The execution owner runs these serially and preserves their
receipts. For a prepared campaign, the namespace preflight consumes no model assignment:

```sh
python3 scripts/agent-eval/review_campaign.py run "$CAMPAIGN_DIR" \
  --preflight-only --limit 1
```

Every actual assignment also attests its application runtime under the real tool permissions
before model invocation. A host-installed Python or Node and a passing host fixture check are
insufficient substitutes. Runtime absence before a model starts leaves a not-started assignment;
failure after a charged invocation preserves that invocation and its known costs.

Freeze the complete inter-campaign order before execution:

```sh
python3 scripts/agent-eval/large_review_study.py schedule --seed 20261008
```

Store this output once in a new private order record before the first model call. The fixed order
fingerprint is `ade185af846a8a8ae1bcd181517724eb0b99a8e0d48ff9b9bd79ff8a66295fc4`.

Every row selects an already prepared `(campaign, case_id, repeat_id, variant)` assignment.
`review_campaign.py run CAMPAIGN --run-id RUN_ID ...` executes that exact assignment through the
existing runner and refuses unknown IDs or a combined batch limit. It never reruns a started
assignment. Heldout native/original/improved runs are adjacent in balanced seeded triplets.
This is the planned order: the selector itself does not enforce cross-campaign sequencing.
Retain an actual-order attestation bound to the frozen order, campaign plans and each existing
controller start/outcome receipt. Use those identities to audit execution order, not elapsed time
to judge efficacy.

Resolve the next frozen row to the unique matching assignment in its campaign's `plan.json`, then
use that assignment's `run_id`. The authentication path is private controller input and must never
be printed, added to a fixture or archived with sources:

```sh
python3 scripts/agent-eval/review_campaign.py run "$CAMPAIGN_DIR" \
  --run-id "$ASSIGNED_RUN_ID" --auth-file "$PRIVATE_AUTH_FILE"
```

The first preregistered development pair is a sequential measurement pilot. Continue based on valid
isolation, consumable answers and understood accounting, not whether a condition wins. Finish the
initial development stage, diagnose its transcripts, freeze the one candidate, finish development
after, and only then execute the heldout triplets. A started assignment is never silently replaced;
a protocol correction requires a separately identified condition and preservation of the original
failure, rather than a favorable extra attempt within this 36-assignment study.

Filesystem, process and network namespaces expose only the selected synthetic repository, copied
pinned runtimes and permitted tools. Host home, development checkout, peer assignments, future
heads and private oracles are absent. The protected controller authenticates separately. Network
qualification requires both a distinct tool/controller namespace and an observed denied socket
attempt; an ordinary routing failure is inadequate. Fresh assignment environments also separate
local RepoScout caches. Within-episode reuse remains available; provider-side caching is observed
through counters rather than assumed cold or controlled. See the
[isolation contract](codex-review-evaluation.md#isolation-and-lifetime) for the full boundary.

Native review remains competent and unconstrained by an artificial reading budget. The treatment
adds the exact canonical skill and CLI to the same task and runtime. It does not impose a required
tool route or count non-use as a quality failure. Trace-supported routing, appropriate reuse,
fallible help/query calls and repeated reads are observations whose complete episode costs count.

Blinding is best effort: heldout construction and answers are separated from the tuning owner until
candidate freeze, and quality-adjudication packets omit arm labels and measured costs. A command
transcript or answer wording may still reveal the condition; do not claim perfect blinding.
Adjudicate supported behavior and impact without access to token totals, retain actual reviewer
identities, and preserve superseded judgments. No product or skill change prompted by heldout
outputs belongs to this confirmation.

## Reporting

Export each immutable campaign privately outside Git through the report/export commands, with independently
adjudicated quality. Then pass the four private detailed results to `large_review_study.py report` using
`--development-original`, `--development-improved`, `--holdout-improved` and `--holdout-original`.
The join binds case/repeat, source, oracle, task, runtime and harness identities; campaign identity
disambiguates reused run IDs. The original-only heldout export intentionally has no within-campaign
pair table. It joins the shared native arm only in the study report.

Use new private packet directories and new private export directories, retaining their integrity
records. `ADJUDICATIONS_FILE` contains the independent verdicts bound to packet and answer hashes:

```sh
python3 scripts/agent-eval/review_campaign.py packets "$CAMPAIGN_DIR" "$PACKET_DIR"
python3 scripts/agent-eval/review_campaign.py export "$CAMPAIGN_DIR" "$EXPORT_DIR" \
  --adjudications "$ADJUDICATIONS_FILE"

python3 scripts/agent-eval/large_review_study.py report \
  --development-original "$ORIGINAL_DEVELOPMENT_EXPORT/results.json" \
  --development-improved "$IMPROVED_DEVELOPMENT_EXPORT/results.json" \
  --holdout-improved "$IMPROVED_HOLDOUT_EXPORT/results.json" \
  --holdout-original "$ORIGINAL_HOLDOUT_EXPORT/results.json"
```

Report the four requested fields separately, including their usage basis and the number of runs
with known whole-episode values. Loading the skill and references, help, query errors, native
fallbacks, rereads and final reasoning/output all belong to the assigned episode.

| Export field | Meaning and interpretation |
|---|---|
| `input_tokens` | Observed input, including cache-read input once. Do not add cache-read again. |
| `cache_write_input_tokens` | Separately observed cache-write input; absent means unknown, not zero. Do not assume an additional disjoint input partition. |
| `cached_input_tokens` | Observed cache-read input, already included in `input_tokens`. |
| `output_tokens` | Observed output, including reasoning already counted by the runtime. Do not add reasoning a second time. |

Do not sum overlapping fields, substitute source-byte estimates, infer subscription charges or
present emitted thread usage as a complete provider-call ledger. When an invocation cannot be
attributed completely, retain safely attributable prefixes as partial observations with their
scope and stop reasons; never relabel them complete episode costs. The all-assigned report keeps
whole-episode known sums and non-overlapping partial known sums separate.

All 36 assignments remain primary, including failed, not-started and unmeasurable outcomes.
Primary costs and quality stay separated by campaign and condition; missing comparison identities
never qualify as an equal match. Quality-matched comparisons are conditional and follow the complete
assignment table. Keep real defects, severity/impact, harmful misses and false findings, causal gaps,
unsupported blockers, wrong-revision evidence and invented execution separate. A harmful miss or
blocker repaired by supported reasoning can constitute a material quality gain; extra wording or
more cited files cannot. At retained or better supported quality without added harmful false
positives, fewer complete tokens are preferred. A material quality gain with modest extra tokens is
an explicit tradeoff, not an automatic weighted score. Mixed token-field changes remain mixed.

Development before/after is exploratory; contemporaneous heldout comparisons distinguish the
original from the improved treatment. Two repetitions show observed variation but establish neither
population-wide superiority nor equivalence. No duration, throughput or speed ranking contributes
to efficacy: timestamps and deadlines support supervision and order auditing only. No remote
publication, PR modification, global installation or historical-result rewrite is part of this study.

Heldout fixtures should differ in application, source context and failure families.
Shared mechanisms limit generalization even when the exact source is unseen.
