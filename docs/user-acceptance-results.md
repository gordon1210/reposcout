# User acceptance results — 2026-10-06

The five [user requirements](user-acceptance-cases.md) were frozen in signed commit `1e44d31`
before their authors inspected current implementation, tests or outputs. Three Astra Perfectionist
agents implemented the cases, a separate methodology reviewer challenged their independence, and
the main agent reviewed and executed them serially. Production code was not changed.

## User outcomes

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

## Confirmed evidence gap

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

The positive assertions remain failing, with named missing obligations. There is no expected-failure
wrapper. A future correction should supply bounded, attributable module context while preserving
source budgets, identity and trust boundaries; moving fixture imports inside functions is not a fix.

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

The last two commands currently fail on the unmet source-binding criteria. `--keep-failed` prints
the retained synthetic repository and private state directory. State includes exact CLI arguments,
stdout, stderr, exits and independent application probe scripts/results. These are complete local
reproducers; no downloaded corpus or old binary is required.

All scenarios remain ignored in ordinary `cargo test` and current CI. A future manual Actions job
can run the same commands and retain failure artifacts. This work does not establish review-quality
or model-token savings, and it adds no automatic agent allocation or tokenizer selection.

## Validation of this patch

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
