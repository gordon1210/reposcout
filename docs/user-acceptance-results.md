# User acceptance results — 2026-10-06

The five [user requirements](user-acceptance-cases.md) were frozen in signed commit `1e44d31`
before their authors inspected current implementation, tests or outputs. Three Astra Perfectionist
agents implemented the cases, a separate methodology reviewer challenged their independence, and
the main agent reviewed and executed them serially. Production code was unchanged for the baseline
recorded in commit `159b526`; the correction below has separate validation status.

## Baseline user outcomes

| User need | Observed result |
| --- | --- |
| A. Review a refund-boundary PR | Sufficient pinned policy, active callers/registration and genuine regression evidence, including a third caller and renamed paths. A separate negative control rejects a test retargeted to an unrelated implementation. |
| B. Review a deleted helper | Old implementation, migrated invoice, remaining refund import/call and deleted target are available. Repairing the refund changes the evidence correctly. |
| C. Investigate the shipping response | Relevant files and function bodies are found, but the active route table and module-level import bindings are missing. All four positive variants remain RED. |
| D. Centralize repeated tariff logic | Real tariff bodies and assertions are found, but source establishing their module-level bindings is missing, including after centralization. Four positive variants and the counterfeit-binding control remain RED. |
| E. Reject newly copied tariff logic | The gate identifies the new tariff locations even when a larger old duplicate is removed. Repair restores a passing gate while allowed legacy debt remains. |

There are 18 tests, including variants and negative controls: nine passed and nine failed. That is
three satisfied user needs and two unmet needs in these fixtures, not nine separate product bugs.
Passing a counterfeit control is not successful completion of a positive user task.

## Evidence gap confirmed on the baseline

The worktree `read` and `plan` source interfaces select declarations. In the natural Python
applications, the route table and important imports are at module scope. The shipping route finds
the request-level test, follows its dependency graph to all five necessary files, and delivers
their function bodies within the six-file/160-line budget. It still cannot deliver, for example:

```python
from service.quote_api import quote_shipping
ROUTES = {("POST", "/shipping/quote"): quote_shipping}
```

The cleanup route also attempts a hash-bound `read --line PATH 1`; a module import is not an
enclosing declaration, so that does not supply the required binding. The counterfeit cleanup
control requires the actual shadow import to be visible; absence of every import cannot count as
successful discrimination.

This is a capability gap relative to the frozen user needs in these public source-query workflows.
It is not evidence of a newly introduced regression, a security issue, or an inability to inspect
the repository using ordinary external file tools. Review-context's opt-in whole-file source
provides the required module evidence in the PR cases. The tests do not manufacture a PR history
for an unrelated worktree investigation to work around the gap.

The baseline positive assertions failed with named missing obligations. There is no expected-failure
wrapper. A correction must supply bounded, attributable module context while preserving source
budgets, identity and trust boundaries; moving fixture imports inside functions is not a fix.

## Validated correction

The correction adds explicit `read --file FILE` through the existing capture and query
pipeline. It returns complete captured file text or an output-budget omission, with the same
snapshot, hash, path-policy and input-limit checks. Existing compact queries do not expand their
source automatically, and tokenization remains the user's default/configuration/CLI choice.

Only the C/D retrieval drivers change: C replaces its definition-plan source request with a batched
file read over graph-discovered paths; D replaces repeated symbol and line-1 reads with one file
selector per discovered path. Both retain three CLI calls. Available expected hashes are preserved.
Fixtures, independent probes, success criteria, source provenance checks and source budgets are
unchanged. All 18 acceptance tests now pass, including the nine previously failing cases and
their negative controls. The original RED results above remain the baseline evidence.

Independent static review found no remaining blocker after a token-only output-omission regression
was added. The 11 new public-CLI regressions exercise complete module context, token/byte budgets,
deduplication, empty and malformed files, stale hashes, worktree/index/tree separation, historical
reads through changed live parents, no-follow policy and protected outputs. Existing query and
journey checks also pass; see the validation record below.

### Bounded output-size comparison

For three retained synthetic fixture states, the new read used only paths/hashes from the saved
public discovery responses. Every returned file hash and snapshot matched the baseline identity.
The first two saved responses were reused unchanged; only the third response was replaced. Thus
this measures serialized output size for these exact states, not a fresh end-to-end timing run.

| Fixture state | All three stdout responses before | After | Reduction |
| --- | ---: | ---: | ---: |
| C, original shipping application | 18,020 bytes | 13,509 bytes | 25.03% |
| D, before centralization, renamed-path variant | 35,594 bytes | 32,947 bytes | 7.44% |
| D, after centralization | 29,648 bytes | 27,842 bytes | 6.09% |

The complete-file reads add the missing module context: source bodies grow from 617 to 861 bytes,
3,651 to 3,768 bytes, and 2,862 to 3,045 bytes respectively. Lower metadata overhead still reduces
the complete output, with three CLI calls in each route. The before-centralization comparison
uses the unchanged renamed variant because the ordinary retained fixture had already been
centralized. No model session was run; these figures establish neither model-token/cost savings
nor improved review quality, and cannot predict output size on other repositories.

### Validation of the correction

All commands ran sequentially against bounded synthetic inputs with the shared two-worker CLI
configuration. Rust checks used `--release --locked -j 1`; test execution retained the serialized
test harness. Times below are observed wall time including the local monitor, not CI guarantees.

| Check | Result | Time | Peak process-tree RSS |
| --- | --- | ---: | ---: |
| Release build / refreshed `reposcoutdev` | Passed | 154.5 s | 1,550 MiB |
| All-target Clippy, warnings denied | Passed | 13.3 s | 1,281 MiB |
| All Rust test targets, compile only (`--no-run`) | Passed | 215.7 s | 1,731 MiB |
| Independent acceptance suite | 18 passed | 8.8 s | 187 MiB |
| Affected public query integration tests | 121 passed, including 11 new file-read tests | 48.7 s | 238 MiB |
| Query unit tests | 20 passed | 0.5 s | 119 MiB |
| Existing response-driven journeys | 7 passed | 16.2 s | 217 MiB |
| Ordinary development-scenarios invocation | All 45 ignored | 0.3 s | 19 MiB |

Compiler validation initially hit the repository's 1 GiB limit and stopped. The user then approved
a task-only exception: up to 3 GiB RSS / 10 minutes for compiler commands, with one Cargo job,
lower scheduling priority and a monitor stopping our owned process tree if system `MemAvailable`
fell below 12 GiB. Compiler commands required at least 15 GiB available before launch; one
preflight correctly refused to launch until memory recovered. The lowest sampled availability
during successful validation was 19,350 MiB. Test and CLI execution retained the ordinary
1 GiB / 180-second limits. This exception does not change repository validation policy.

Formatting, skill-mirror, relative documentation-link, shell-syntax and whitespace checks pass.
The full production Rust suite was compiled but not executed; affected targets were executed as
listed above. The unchanged original 20 focused scenarios, frontend checks and macOS execution
were not rerun. No workflow or dependency changes are needed; schema 2.0 and analyzer version 24
remain unchanged because the new selector reuses existing captured facts.

## Independent truth and sensitivity

- Refund flows are actually executed at days 13/14/15: `[true, true, false]` becomes
  `[true, false, false]`. The real day-14 assertion fails at head; an unrelated replacement passes.
  A later local repair cannot replace requested PR evidence.
- The deleted-helper fixture executes both base flows; at head the forgotten refund specifically
  fails to import `money`. The external same-name distractor remains runnable.
- The shipping dispatcher returns `[499, 499]` with the defect and `[0, 499]` after the authored
  repair. The request-level assertion changes from failing to passing. Route-switch and decoy
  variants preserve their independently specified obligations; current source is checked after
  cached discovery.
- Tariff entrypoints execute all 12 frozen price examples and nine invalid-input cases before and
  after centralization. A third copied implementation grows maintenance responsibility; a delegated
  caller does not. Both cleanup phases execute before missing evidence is asserted.
- The debt replacement case requires actual new tariff finding locations and their source, plus
  separately resolved legacy locations. A net count or ratio cannot satisfy it.

During test development we corrected harness errors, including interpreting an abbreviated
signature (`def ...: …`) as literal source. We also rejected a fixture layout that moved imports
inside functions merely to make source retrieval succeed. Neither is counted as a product finding.

## Reproduce

Requires a Unix Rust environment and Python 3; application probes use only the standard library.
Each command runs real one-shot processes serially. No model, server or network fixture is involved.

```sh
./scripts/test-scenarios.sh acceptance::review
./scripts/test-scenarios.sh acceptance::health::debt
./scripts/test-scenarios.sh --keep-failed acceptance::investigation
./scripts/test-scenarios.sh --keep-failed acceptance::health
```

On the baseline, the last two commands failed on unmet source-binding criteria; the corrected
drivers now pass without changing those criteria. `--keep-failed` prints the retained synthetic
repository and private state directory when a case fails. State includes exact CLI arguments,
stdout, stderr, exits and independent application probe scripts/results. These are complete local
reproducers; no downloaded corpus or old binary is required.

All scenarios remain ignored in ordinary `cargo test` and current CI. A future manual Actions job
can run the same commands and retain failure artifacts. This work does not establish review-quality
or model-token savings, and it adds no automatic agent allocation or tokenizer selection.

## Validation of the baseline test patch

The final combined acceptance run produced the same nine passes/nine criterion failures in 54.71 s
after compilation, with a monitored 189 MiB process-tree peak. The seven existing command-chain
journeys also passed after the shared helper change (83.53 s, 239 MiB). These are local observations,
not CI timing guarantees. The normal integration-target invocation ran no scenarios and reported
all 45 ignored; the documented runner listed exactly 18 acceptance tests and refreshed release.

Formatting, focused Clippy for the complete development-scenarios target with Cargo's lint flags,
test-target compilation, release build, shell syntax, local documentation links and whitespace
checks passed. The final compilation stayed below the 1 GiB limit (873 MiB). The full production
Rust suite, all-target Clippy, frontend checks and macOS execution were not rerun for this test-only
correction; the original 20 focused scenarios were unchanged and their earlier results remain in
the scenario guide. Production source, schema/analyzer versions, dependencies and workflow files
are unchanged.
