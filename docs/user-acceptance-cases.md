# User acceptance cases: requirements before CLI routes

These requirements were agreed on 2026-10-06 by three independent domain designers and a
methodology reviewer **before they inspected RepoScout's implementation, existing scenarios or
outputs**. They correct an earlier mistake: chaining valid CLI responses does not by itself
demonstrate that a user's information need has been met. The CLI is the means, not the oracle.

## What counts as success

Each case starts with a user's decision and an independently authored miniature application.
Business examples, active entrypoints and expected changes establish truth. Small one-shot fixture
executions can confirm that truth; RepoScout is not expected to execute, diagnose or repair the
application. Tests measure whether sufficient truthful evidence reaches the user.

There are three distinct outcomes:

- Sufficient, correctly attributed evidence: the positive acceptance case passes.
- Honest but insufficient evidence: the positive case fails, with the missing obligation named.
- Wrong or misleading evidence: the case fails, even if other useful evidence was returned.

A separately specified negative control can establish that a counterfeit test or missing target
does not satisfy an obligation. It must not be reported as successful completion of the positive
user task. Do not add expected-failure wrappers or relax requirements when a product gap appears.

Drivers receive only stated user inputs and a public CLI execution handle. They may consume
returned paths, symbols and identities, but cannot access the oracle's expected targets, read the
fixture directly or backfill missing evidence. Different valid CLI routes and modest relevant extra
context are allowed. Relationship claims must be supported by real bindings/calls, not names alone.

Each variant is bounded to 20 source/data files, 48 KiB and eight sequential CLI invocations. These
are resource safeguards, not definitions of usefulness. Existing child timeouts, shared two-worker
configuration and serial execution apply. Only the shipping task has a user reading budget.

## A. Review a refund-boundary change

**User input:** repository and base/head revisions; "Refunds are permitted through day 14. Prepare
the evidence to review this change, including affected production entrypoints and regression checks."
No internal paths or symbol names are supplied.

**Independent truth:** two active production flows, including an aliased binding, use a policy whose
predicate changes from `days <= 14` to `days < 14`. For days `[13, 14, 15]`, both flows return
`[true, true, false]` at base and `[true, false, false]` at head. A direct day-14 assertion calls the
real policy and passes at base but fails at head. An unrelated same-name definition is a distractor.

**Necessary evidence:** the exact before/after policy source from the requested revisions; both
production callers with their actual binding/call connections; the genuine day-14 assertion and its
connection to this policy. A useful test filename or a similarly named unrelated function cannot
satisfy these obligations. A later worktree repair cannot replace requested historical evidence.

**Sensitivity:** adding a third active aliased caller grows the required caller set. Renaming files
and adding unused homonyms preserve the semantic obligations. Retargeting the test to an unrelated
implementation removes genuine regression evidence even when its name and path stay unchanged.

## B. Review deletion of a shared helper

**User input:** repository, base/head revisions and a request to review a rounding-helper migration
for remaining affected consumers; no internal paths.

**Independent truth:** invoice and refund entrypoints execute the old local helper at base. Head
deletes it, migrates invoice to a replacement, and leaves refund importing/calling the absent local
target. Invoice still runs; refund deterministically fails resolving that target. A same-name
external distractor is independently runnable, so its own failure cannot establish this truth.

**Necessary evidence:** the old implementation at base, migrated invoice source at head, refund's
remaining local import/call at head, and absence of that local target at head. An empty consumer
result after deletion does not establish absence of affected consumers.

**Sensitivity:** repairing refund's binding removes its unresolved-consumer obligation while
preserving the old-implementation and migration evidence. The external distractor remains valid.

## C. Investigate a shipping support ticket

**User input:** `POST /shipping/quote`, request `{delivery_pass: true}`, observed
`shipping_fee_cents: 499`, and the business rule that pass holders pay zero; false is the 499 control.
The user asks for evidence within **six source files and 160 nonblank source lines**. No implementation
path or symbol is supplied.

**Independent truth:** the active route dispatches a handler which calls a tariff rule and a
renderer. The rule returns zero for a pass and 499 otherwise. The renderer uses `fee or 499`, losing
the valid zero. A request-level regression test dispatches the actual route and asserts literal
responses. Same-name demo/archive code and large prose are distractions. A stdlib one-shot probe
returns `[499, 499]`; a separately authored None-only renderer repair returns `[0, 499]`.

**Necessary evidence:** active route binding and dispatch; handler's rule-to-renderer connection;
the zero-fee rule; the faulty serializer expression; the request-level test's route, request inputs
and literal assertions. A calculator-only test is insufficient. Every emitted source fragment
counts against the user budget, including irrelevant or repeated material; the oracle cannot
select a convenient subset afterward.

**Sensitivity:** switching the route to an alternate handler/renderer moves required provenance
while old implementations remain decoys. Decoy-only edits preserve obligations. A renderer repair
requires current corrected source and must reject stale bug evidence.

## D. Centralize a repeated tariff rule

**User input:** shipping-fee domain, owned `src/` tree, and a request to identify where one tariff
rule must be maintained independently and what regression evidence can support centralization.
The user does not know duplicate locations.

**Independent truth:** checkout and batch invoicing contain independent implementations of the same
tariff business table. Actual assertions invoke the relevant implementations. A larger generated
bundle, repeated data and an unrelated real-code clone pair are distractions. The authored repair
centralizes the tariff rule while preserving both entrypoint results and the unrelated clone debt.
Functions must be naturally written from the business rules, never padded to detector thresholds.

Frozen tariff precedence, all amounts in cents: domestic starts at 499, international at 999;
weight above 2000 g adds 200. For domestic only, order value at least 5000 or a delivery pass resets
the subtotal including weight to zero. Express adds 300 last, including to free shipping.
International has no value/pass exemption. Inputs are an integer value >= 0, integer weight > 0,
one of the two regions and boolean options; invalid inputs are rejected.

| Region | Value | Weight g | Pass | Express | Expected cents |
| --- | ---: | ---: | --- | --- | ---: |
| domestic | 4999 | 2000 | false | false | 499 |
| domestic | 5000 | 2000 | false | false | 0 |
| domestic | 4999 | 2001 | false | false | 699 |
| domestic | 5000 | 2001 | false | false | 0 |
| domestic | 4999 | 2001 | true | false | 0 |
| domestic | 4999 | 2001 | true | true | 300 |
| domestic | 5000 | 2001 | false | true | 300 |
| international | 4999 | 2000 | false | false | 999 |
| international | 5000 | 2000 | false | false | 999 |
| international | 5000 | 2000 | true | false | 999 |
| international | 5000 | 2001 | true | false | 1199 |
| international | 5000 | 2001 | true | true | 1499 |

**Necessary evidence:** both active maintenance sites with source showing their tariff responsibility
and production role, plus real tariff assertions and their binding to those implementations. Do not
recommend editing generated/data copies. After the authored repair, the targeted independent-copy
obligation is gone while unrelated duplication remains. Wrappers are not independent tariff copies.
Source evidence alone does not prove behavioral safety; fixture execution validates the authored
repair separately.

**Sensitivity:** a third independently maintained tariff implementation expands the obligation;
a third delegated caller does not. Path renames and generated/data ballast preserve substance.
A same-path test retargeted to an unrelated implementation cannot satisfy useful-test evidence.

## E. Reject new tariff debt despite unchanged totals

**User input:** a baseline allowing existing unrelated formatting duplication, and a requirement to
reject a newly copied shipping tariff rule.

**Independent truth:** base has one shared tariff implementation and a legacy unrelated clone pair.
Head adds an independent tariff implementation in a billing entrypoint. The decisive variant also
removes the unrelated clone pair: improved aggregate totals must not hide this new obligation.

**Necessary evidence:** identify the specific newly independent tariff site and distinguish it from
permitted legacy debt. A regression gate fails for the new copy and passes when that copy is removed.
No global zero-duplication requirement is imposed; a net count or ratio alone is insufficient.

## Scope of the suite

These are deterministic opt-in tests of real CLI use, not a model/agent harness or an evaluation of
human review quality. Existing inventory, budget accounting, JSON, cache, filesystem and command-chain
tests remain useful contract checks, but cannot stand in for these acceptance criteria. No automatic
CI workflow, tokenizer selection or agent allocation follows from this design.

Requirements may change only for an independently justified user/domain correction, recorded openly.
Observed RepoScout behavior is not such a justification. Execution results and any product gaps belong
in a separate report so the original requirements remain reviewable.
