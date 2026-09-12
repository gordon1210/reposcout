# Agent evaluation: 2026-09-12 native pilot

All 38 bounded retrieval and behavior answers passed their checks, but this pilot does
**not establish net token savings**. All six comparisons after the single
routing-policy revision consumed more total model tokens in the RepoScout arm. The separately
prescribed `changes → consumers → read` workflow also consumed more. These results remain part of
the evidence; successful functionality does not turn an adverse token result into a saving.

The [sanitized results](../scripts/agent-eval/results/2026-09-12-native-pilot/results.json) contain
all 38 runs, 19 comparisons, usage components, actual invocations, quality checks, policy snapshots,
source-exposure coverage and prompt provenance. The accompanying
[integrity manifest](../scripts/agent-eval/results/2026-09-12-native-pilot/integrity.json) identifies
canonical-JSON hashes. The [evaluator guide](../scripts/agent-eval/README.md) explains the contracts.

This is historical evidence for the analyzer-20 development binary captured before the subsequent
review fixes. It is **not a measurement of the 0.3.0 release binary**; release validation does not
retroactively rerun or change these observations.

## What was measured

Each condition has **one run per arm**, using a bounded synthetic Rust repository with fixed
source, task and answer-oracle hashes. The tasks find definitions and evaluate their behavior;
they do not implement repairs, run compiler/test suites or establish regression safety for code
changes. The baseline can use competent `rg`, diff hunks and targeted source reads, including
refinement or justified broader reads. Actual work is retained even when a run chooses an
inefficient command; the baseline is not replaced retrospectively with an ideal command trace.

The native harness used Codex CLI 0.154.0, `gpt-6-astra` at medium effort and fresh task sessions
without child-agent execution. The captured starting context, task/prompt/policy hashes and
binary identity are recorded per run. The measured binary SHA-256 is
`f75db15bec71bbec79c2cc2cacbccb9b9bf5c5fd95d9d4603ab6d1d725c6e9cf`.
It is a local development build, not a new published release.

Total tokens are actual input plus output over the entire measured session. Input includes
uncached, cache-read and cache-creation tokens exactly once; reasoning is a subset of output,
not an additional charge. Repeated cumulative usage updates are deduplicated. Tool output bytes
are reported separately and are not converted to guessed tokens or prices. Real CLI discovery,
help, retries and source reads remain in the total. A large cached starting context makes extra
turns materially affect this metric; total tokens are not equivalent to monetary cost.

## Original conditions

The original campaign has six isolated feature pairs and six end-to-end pairs. **Every original
end-to-end RepoScout-available arm used only native tools.** Those rows measure availability and
natural tool choice, not the effect of executing a RepoScout query. The negative F6 end-to-end
delta therefore cannot be attributed to RepoScout retrieval.

Positive deltas mean more tokens in the RepoScout arm. All rows passed their answer oracle.

| Task | Baseline tokens | RepoScout-arm tokens | Delta |
|---|---:|---:|---:|
| F1:known-definition:end-to-end | 111,884 | 112,101 | +217 |
| F1:known-definition:isolated | 83,564 | 84,054 | +490 |
| F2:changed-definition:end-to-end | 111,453 | 136,278 | +24,825 |
| F2:changed-definition:isolated | 136,750 | 85,125 | -51,625 |
| F3:unknown-entry:end-to-end | 110,885 | 111,160 | +275 |
| F3:unknown-entry:isolated | 110,878 | 112,304 | +1,426 |
| F4:multi-location-diagnostics:end-to-end | 111,962 | 112,151 | +189 |
| F4:multi-location-diagnostics:isolated | 112,512 | 117,229 | +4,717 |
| F5:definition-plan:end-to-end | 152,388 | 153,100 | +712 |
| F5:definition-plan:isolated | 140,393 | 142,909 | +2,516 |
| F6:consumers:end-to-end | 166,157 | 111,739 | -54,418 |
| F6:consumers:isolated | 111,756 | 120,706 | +8,950 |

F1's isolated task already supplied an exact, complete three-line range: the native read was
cheaper without needing snapshot, hash or coverage evidence. F2's favorable isolated result
includes an actual broad baseline search returning roughly 64 KB after a small diff. Its adverse
end-to-end result includes a broad native read in the RepoScout-available arm. These observations
explain particular traces; neither licenses a universal efficiency claim.

## One routing-policy revision

The next six matched pairs received compact, general evidence-based routing guidance, with the
actual prompt cost included. Known small spans and tiny logs can use native reads; known diffs
can use bounded changes; unknown entries can use find; plans serve budget competition; consumers
answer proven relationship questions. The baseline received matching bounded-search guidance.
These are new conditions, not repeated samples under the original prompts. No further
optimization cycle was run after their adverse results.

| Task | Baseline tokens | RepoScout-arm tokens | Delta |
|---|---:|---:|---:|
| F1:known-definition:isolated | 83,588 | 84,154 | +566 |
| F2:changed-definition:end-to-end | 112,099 | 147,358 | +35,259 |
| F3:unknown-entry:isolated | 111,317 | 220,963 | +109,646 |
| F4:multi-location-diagnostics:isolated | 112,868 | 146,488 | +33,620 |
| F5:definition-plan:isolated | 112,142 | 173,891 | +61,749 |
| F6:consumers:isolated | 112,249 | 193,172 | +80,923 |

Discovery cost is visible: routing F3 used five RepoScout invocations, of which three were help
and two were actual `find`/`read` queries. Routing F6 used three invocations, of which two were
help and one was `consumers`, plus native `rg`. Counting all invocations as repeated substantive
queries would misdescribe the evidence. All six routing comparisons nevertheless remain net
increases under the measured full-session accounting.

## Separate integration workflow

This pair explicitly required the RepoScout arm to discover changed identities, follow incoming
consumers with current-side hashes, and read the necessary caller/helper definitions. It is a
prescribed integration-workflow measurement, not free routing and not another original feature
row. Both arms produced the expected behavior values `[0, 900]`.

| Task | Baseline tokens | RepoScout-arm tokens | Delta |
|---|---:|---:|---:|
| COMPOSED:changed-consumers-source:end-to-end | 142,440 | 337,131 | +194,691 |

The baseline used four tool calls and the RepoScout arm nine. The latter made seven RepoScout
invocations: three help calls and four real queries (`changes`, `consumers`, `read`, `read`).
The workflow is functional, but this pair does not satisfy the goal of replacing more model work
than it adds.

## Source delivery and latency

Source exposure matches actually delivered tool output to captured logical paths and content
hashes. It does not prove that the model retained or used those bytes. Across 38 runs, 561
provenance-range records were attributed; six runs have partial quantitative coverage because
four outputs were truncated and two contained normalized Find signatures. Their values are
**lower bounds**: a zero does not prove absence of reads or overlap. Even fully attributed
supported reads are not a claim of complete model-context coverage. No compaction events were
observed, which still does not establish retention.

In the original F5 isolated RepoScout arm, two external follow-up source calls repeated 97 bytes
already delivered by RepoScout. In the composed pair, the baseline delivered 560 unique observed
source bytes and repeated 243 bytes across results; the RepoScout arm delivered 415 unique bytes
and repeated 117. One external follow-up accounted for all 117 repeated bytes. Less unique source
output in that arm coexisted with much higher total model input/output.

The separate local latency matrix contains 18 successful invocations across six features, with
requested fresh-root, repeated and after-edit phases. Elapsed times were approximately 57–151 ms.
Cache-hit status was not exposed, so these are not verified cold-cache/warm-cache distributions.
There is one sample per feature/phase, not a large-repository or production latency benchmark.

## Provenance and limits

Native response records expose real unique response IDs and explicit usage fields. The evaluator
checks their sequence and accumulated usage against advancing native cumulative updates. IDs do
not themselves prove response finality, complete failure export or absence of unreported children.
Closed whole-session lifecycle and child inventory remain externally attested by the controller;
this is explicitly distinct from the generic final-provider-call ledger contract.

All 38 parent prompt payloads were present but opaque encrypted/Fernet-shaped data. Their
ciphertext hashes are preserved separately from prepared plaintext hashes. The controller
attests that the exact prepared prompts were submitted; plaintext equality could not be
independently verified from those logs. This is a verification gap, not a detected mismatch.
No decryption or key search was attempted. Superseded unrun task/policy revisions remain recorded,
and no adverse executed result was discarded.

All 38 answer checks pass, but there is only one observation per condition, on synthetic fixtures
and one native harness/model configuration. This is not a statistical comparison, a Graft
benchmark, a repair-success study or an exhaustive language/runtime coverage campaign. No pricing
or general percentage-saving claim follows from it.

## Consequences for RepoScout

Keep all six implemented features and their explicit coverage contracts. Use them where identity,
snapshot correctness, change evidence, bounded selection or proven relationships replace a real
piece of work. A known small complete source span or tiny explicit diagnostic location can remain
a native read. A signature-environment plan must not be treated as body-call closure.

CLI discovery should be amortized through concise, accurate skill routing and reusable capability
knowledge. That is a follow-up design direction, not a measured saving from this pilot. Returning
more metadata or fewer source bytes is not sufficient: the acceptance metric remains full-session
work at equal answer quality. The bounded policy correction was tested once and did not establish
that benefit; do not hide this by extending optimization until a favorable sample appears.

M1's accounting/fixture infrastructure and this bounded M2 pilot now have observed evidence.
Broader repair/regression tasks, replicated scenarios and verified cache-state measurements remain
unproven. Feature implementation, completed pilot execution and the product's net-saving objective
are separate statuses; this report establishes the first two within their documented scope, not
the third.

## Local validation state

The measured development tree passed Rust formatting, strict all-target Clippy and 748 Rust tests. The
evaluator passed 28 Python tests, and the final release build preserved the measured binary hash.
Independent export checks matched 168 unique real response IDs and their usage to all 38 closed
run totals. The results canonical-JSON SHA-256 is
`5447ec5e69ba3747e34a76ea65af7d9e6b897589825f7863fc9d63fedf9bd081`.
These are historical implementation/evidence checks for the measured tree, not the final
validation record of subsequent review fixes or release publication.
