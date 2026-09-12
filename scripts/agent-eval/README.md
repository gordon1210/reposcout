# Agent evaluation fixtures and usage accounting

This offline evaluator supports the six-feature RepoScout program. It validates canonical usage
ledgers, prepares bounded task fixtures and checks answers. It does not execute a model, run a
compiler, start an agent or estimate missing provider usage. These scripts are development tools;
RepoScout's product does not gain a model backend or telemetry.

The checked-in usage example is **synthetic**; the reusable fixture manifest's `not-run` state is
not a campaign result. The [observed 38-run pilot](../../docs/agent-evaluation.md) has separate
[results and provenance](results/2026-09-12-native-pilot/results.json). All retrieval/behavior
answer checks passed, but net savings were not established. Evaluator tests validate accounting
rules, not repair success or general efficiency.

## Prepare a bounded task

Run these commands from the repository root:

```sh
# Evaluator-only manifest, including answer oracles and baseline guidance.
python3 scripts/agent-eval/fixtures.py manifest

# Public task payload: excludes the oracle and evaluator baseline instructions.
python3 scripts/agent-eval/fixtures.py task --task F1:known-definition:end-to-end

# Check an independently produced answer against the fixed oracle.
python3 scripts/agent-eval/fixtures.py verify \
  --task F1:known-definition:end-to-end --answer /path/to/answer.json
```

Keep the private evaluator manifest and expected answers out of the agent's starting context.
`fixtures/tasks.json` records the twelve cases: each of the six tasks has `isolated` and
`end-to-end` variants. Fixture source hashes cover the Rust source files, the old inventory
snapshot and the diagnostic input. The diagnostic records are synthetic rustc-shaped warnings,
not evidence that a compiler ran.

| Feature | Task | Quality oracle |
|---|---|---|
| F1 | Read a known quantity-capping definition | Correct definition, location and evaluated boundary values |
| F2 | Identify a changed definition and its behavior | Correct changed definition, location and value from the new side |
| F3 | Find the retry-delay implementation from a task description | Correct definition and source evidence |
| F4 | Resolve multiple diagnostic locations | Both relevant definitions and their source evidence |
| F5 | Plan an invoice definition and its direct dependencies | Main definition and required local/imported dependencies |
| F6 | Identify direct invoice consumers | Both direct callers with source evidence |

The isolated variant fixes the task entry selectors, separating retrieval from target discovery.
Those selectors must not disclose the oracle's discovered answer set. The end-to-end variant
includes the real entry-point problem. Never combine their costs as if they
were identical tasks. The baseline uses bounded `rg` searches followed by targeted `sed` reads,
permits query refinement, and permits a whole-file read when the task justifies it. It does not
force an inefficient full-file baseline or invent search retries that never happened. Execute the
baseline commands in `fixtures/repository`, using the evaluator's plan without exposing its oracle
to the agent.

Answers are JSON objects containing `definitions` and `evidence`, plus `values` for behavior tasks.
Each evidence item must match the required `path`, `symbol`, `start_line` and `end_line`. Verification
reports `passed`, `regressions`, `missing_evidence` and the answer's evidence hash. Answer input is
limited to 1 MiB. These are retrieval and behavior-answer quality checks. They do not apply code repairs, run a
regression benchmark or establish correctness on general repository repairs or large refactorings.
Report RepoScout availability separately from observed use: a successful arm that never invokes
RepoScout does not measure its query effect. Preserve adverse outcomes from the single approved
routing-remeasurement cycle rather than selecting only favorable repeats.

## Fix the comparison conditions

Each manifest uses schema `1`, a unique `run_id`, `variant` (`baseline` or `reposcout`), and the
following shared conditions:

- `task_id`, `task_sha256`, `fixture_sha256` and `oracle_sha256`;
- exact `provider`, `model` and `harness` identifiers;
- `start_context_sha256` and `retention_policy`;
- `mode` (`isolated` or `end-to-end`) and `evidence_kind` (`synthetic` or `observed`).

Hashes must be lowercase SHA-256 strings. Record model and harness versions precisely enough to
reproduce the comparison, including configuration in the pinned identifiers or retained evidence.
The canonical format requires one provider/model condition per scenario, including its subagents;
it does not silently aggregate mixed-model runs into a comparable result.

Retain the actual provider export and document its field mapping. Every canonical usage record
requires `provenance.raw_sha256`, `mapping_id` and `mapping_reference`. The ledger checks their
presence and hash shape; it does not fetch the raw export, verify its contents, or prove that the
export contains all calls. The canonical ledger has no provider-specific final-call adapter. The separate native Codex
window adapter below audits supported cumulative records and, where present, real response IDs
without inventing final-call status or export completeness. A missing or
ambiguous provider field needs a supported normalization decision, not an invented zero.

## Record usage exactly once per provider call

Events are JSONL objects with schema `1`, matching `run_id` and unique `event_id`. Register the root
agent first, then children with an existing `parent_agent_id`. A single final `usage` record per
provider call identifies `agent_id`, `attempt_id`, `provider_call_id`, provider/model and call
status (`success`, `failed` or `aborted`). Streaming deltas and cumulative updates must be normalized
before entering this ledger; duplicates are rejected, not summed.

Input tokens form a disjoint partition:

```text
input_tokens = input_uncached_tokens
             + input_cache_read_tokens
             + input_cache_creation_tokens

total_tokens = input_tokens + output_tokens
```

All five fields are required nonnegative integers. Normalize output to include reasoning tokens
exactly once. Optional `reasoning_tokens` is a subset of `output_tokens`, never an extra summand;
use null or omit it if that subset is unknown. Cached input still contributes to total model
context, but appears only once. Optional provider costs use a nonnegative decimal string and
currency and remain separate from token counts.

Preserve failed calls, retries, subagents and aborted tasks with their real usage. A mandatory
`quality` event records a boolean result, regression and missing-evidence lists, and evidence hash,
even for a failed task. The final `run_end` event supplies status, `usage_complete`, and exact agent
and provider-call inventories. No event may follow it. Inventory consistency detects internal
omissions; `usage_complete: true` still relies on a trustworthy external export and cannot prove
that unknown calls were included.

The parser rejects duplicate JSON fields, duplicate event/call IDs, foreign runs, unknown agents,
invalid input partitions, missing final records and unsupported event kinds. JSON inputs and the
whole event stream each have a 16 MiB limit; at most 100,000 events are accepted.

## Keep explanatory measurements separate

`tool` events record output bytes and latency grouped as `cold`, `warm`, `after-edit` or `other`.
`source` events record half-open byte ranges keyed by path and source hash, including an explicit
`followup` flag. Unioning ranges with the same content identity exposes repeated source bytes;
different content hashes are distinct even at the same path. `context` events preserve observed
retention or compaction information.

These records explain where work was spent. Do not add tool/source bytes or locally estimated
source tokens to provider input totals: any content actually sent to a model is already represented
in its real call usage. Source-range overlap is not proof that the model retained the earlier
content, and a local analysis-cache hit is not a provider-cache saving. Provider costs and latency
also remain distinct from net model tokens.

## Summarize and compare

```sh
# Check the synthetic cache/retry/subagent example.
python3 scripts/agent-eval/accounting.py summarize \
  scripts/agent-eval/fixtures/usage-manifest.json \
  scripts/agent-eval/fixtures/usage-events.jsonl

# Summarize each real exported run, then compare the saved summaries.
python3 scripts/agent-eval/accounting.py summarize /path/to/manifest.json /path/to/events.jsonl
python3 scripts/agent-eval/accounting.py compare /path/to/baseline.json /path/to/reposcout.json
```

The synthetic example contains a failed root call (100 input + 10 output), its retry
(80 + 12), and a child call (40 + 8). Totals are **220 input + 30 output = 250 tokens**.
The input partition is 110 uncached + 80 cache-read + 30 cache-creation. Seven reported reasoning
tokens are already inside output; the third call's reasoning subset is unknown. Its 999 tool-output
bytes, 300 source bytes, 250 unique source bytes, 50 repeated source bytes and synthetic USD 0.03
are separate explanatory values. None is a real saving or a real provider charge.

`compare` requires identical comparison conditions and the correct baseline/RepoScout variants.
It emits a token delta only when both summaries are `observed`, usage-complete, successful and
quality-passing, with no regression or missing-evidence entries. The delta is RepoScout minus
baseline, so a negative value means fewer tokens under those exact conditions. Synthetic,
failed, aborted or incomplete runs remain visible but are ineligible for a savings claim.

Use only summaries produced from validated ledgers; the comparison command consumes summary files,
not raw provider exports. Store actual evaluation results with their conditions, quality evidence,
failed attempts and remaining limitations. Missing mandatory runs keep M2 incomplete. Optional
extra cases may remain explicitly unrun; they must not be mixed into measured results.

## Audit native Codex session windows separately

`native_codex.py` supports a distinct accounting basis for native Codex logs. Use only the explicit
session logs and bounded windows authorized for the evaluation. It extracts session/model metadata
and token-usage events, excluding prompt and response payloads from its output.

```sh
python3 scripts/agent-eval/native_codex.py audit /path/to/session.jsonl \
  --session-id SESSION_ID --start-after 0 --end-at TOKEN_EVENT_ORDINAL

python3 scripts/agent-eval/native_codex.py close \
  /path/to/manifest.json /path/to/attestation.json /path/to/quality.json \
  /path/to/root-audit.json /path/to/child-audit.json

python3 scripts/agent-eval/native_codex.py compare \
  /path/to/baseline-window.json /path/to/reposcout-window.json
```

Window ordinals count `token_count` events: the start is exclusive and the end inclusive. The adapter
requires all six native fields (`input_tokens`, `cached_input_tokens`, `cache_write_input_tokens`,
`output_tokens`, `reasoning_output_tokens`, `total_tokens`). It checks that cumulative advances
match the corresponding last-usage records, rejects resets, unexplained gaps and model/effort
drift, and ignores unchanged repeated totals only when their last-usage values also agree. Missing
cache fields are not filled with zero. Reasoning is already inside output; uncached input is input
minus the disjoint cache-read and cache-write portions.

An audit alone always reports `usage_complete: false`. Native 0.154 logs can also supply
`token_usage_record` events with actual response IDs. The adapter validates each record's
`thread_id` against the audited session UUID; a root-valued `session_id` is not substituted for
that child identity. It rejects duplicate response IDs and checks explicit usage fields,
per-response usage against advancing token-count deltas, and accumulated usage against reported
thread totals. It exports `response_usage` and sets `provider_call_ids_available: true` only when
selected response records exist; older logs without them retain false. Response and cumulative
usage are cross-checks, not two additive sources of cost.

Neither a real response ID, EOF nor matching cumulative totals proves final-call status, error
completeness, that the session ended or that every child was included. Bounds
are 256 MiB per log, 16 MiB per line and one million records. Only OpenAI-provider records matching
the requested session identity are accepted by this adapter.

Closing a window requires fresh whole-session audits (`start_after: 0`) plus a separate lifecycle
attestation. It must identify the run, evidence hash/reference, closed state, complete inventory,
status, and every session in parent-before-child order. Each session entry supplies its exact final
ordinal, projection hash and success/failed/aborted status. The adapter requires one root, an exact
match between audits and attested sessions, matching model/effort/provider, valid projection
hashes and consistent token totals. Failed or aborted session usage remains in the sums.

This attestation is external evidence supplied by the evaluation controller. The adapter checks
its consistency; it does not independently establish its truth or manufacture missing provider-call IDs.
The output explicitly identifies its basis as `attested-native-session-cumulative-windows` and
must not be relabeled as the canonical final-provider-call ledger.

Native comparison additionally fixes native CLI versions, requires distinct run identities and
disjoint sessions, and only computes a delta for observed, complete, successful, quality-passing
runs with no missing evidence or regressions. Keep the baseline task, model, start context, fixture,
retention policy and oracle conditions identical. A valid native comparison measures that stated
basis; it does not turn the synthetic fixtures or unrun cases into empirical results.

## Prepare paired native trials

The portable [prompt template](fixtures/prompt-templates.json) contains public task instructions
and variant guidance, not answer oracles. `pilot.py` prepares separate baseline and RepoScout
workspaces for all twelve cases. Before-snapshot and diagnostic inputs live in an explicitly named
`../inputs` directory so they do not pollute repository scans with duplicate source.

```sh
python3 scripts/agent-eval/pilot.py prepare /path/to/new-trials \
  --binary /path/to/verified/reposcout \
  --templates scripts/agent-eval/fixtures/prompt-templates.json

python3 scripts/agent-eval/pilot.py verify /path/to/trial.json /path/to/answer.json
python3 scripts/agent-eval/pilot.py store-final /path/to/trial.json /path/to/answer.json \
  --agent-id SESSION_ID --native-log /path/to/session.jsonl \
  --lifecycle-evidence-sha256 '<SHA256>'
```

Preparation pins the chosen binary hash and creates only task-owned fixture Git state where the
diff case needs it; it makes no commits. It does not start model runs. The public prompt permits
only the source workspace and explicit task inputs, with the answer as the only agent-authored
file. Ordinary tool-managed external cache writes are allowed; cache clearing and configuration
mutation are not. RepoScout uses `--no-project-config`, retaining the existing native global
settings and cache rather than changing HOME or silently creating a different environment.

`store-final` verifies the answer and records quality, session/log pointers and lifecycle evidence.
It does not read the log, import usage or infer completion: stored usage remains `not-imported`
until the separate audit and attestation steps succeed. These trials do not measure a complete
cold/warm/after-edit matrix or correctness of code changes; report those omissions explicitly.

## Sanitized campaign evidence

`native_evidence.py` extracts bounded tool/latency/compaction metadata; `close_trial.py` combines
answer checks with explicit lifecycle evidence. `source_exposure.py` attributes delivered source
ranges and marks partial metrics as lower bounds, without inferring retained context. `latency.py`
records requested fresh/repeat/after-edit runs separately from model usage and leaves unknown
cache-hit status unknown.

`prompt_delivery.py` checks the authorized parent spawn inventory. Opaque encrypted payloads
preserve ciphertext hashes and an explicit controller attestation; they cannot establish
independent plaintext equality. `export_results.py` exports the campaign with `--release`,
`--source-exposure` and `--prompt-audit` evidence into `results.json` and `integrity.json`. Hashes
are labeled as canonical JSON. The export retains original, routing and composed conditions,
superseded unrun revisions and adverse outcomes. It does not publish raw session logs or source
bodies. See the [pilot report](../../docs/agent-evaluation.md) for observed results and limitations.
