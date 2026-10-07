# Two additional user-efficiency requirements

Frozen on 2026-10-07 before implementing either scenario or adapting its public CLI driver.
These extend [cases I and K](user-efficiency-cases.md), preserving their existing numeric budgets,
fixture ceilings, public-response-only navigation and complete-interaction accounting. They are
opt-in development tests, not a model/agent harness or an automatic CI gate.

## I: the actual binding is not in the module preamble

The user still investigates `src/invoices.py::net_due`: a 1,005-cent invoice with a 1,000-basis-point
discount must cost 904 cents under nearest-cent, halves-up rounding, but costs 905. Other literal
examples remain 1000/1000 → 900, 1005/0 → 1005, and 1005/10000 → 0. The same independently
authored integer repair and genuine production-bound regression check apply.

Keep the naturally substantial module, but place its actual model and rounding imports after
existing pagination/export helpers and before `net_due`. Use valid parenthesized multiline
imports. The required imports lie beyond the first 16 physical lines; no padding or artificial
imports inside a function may manufacture the case.

Required evidence remains the actual bindings, `Invoice` units/fields, `net_due`, the active
rounding helper, and a real regression check bound to the production path. Source of a same-named
unconnected helper is insufficient. Freeze exact fragments/spans before the CLI runs; those private
spans cannot enter driver inputs. Public definition locations can guide bounded exploration.

Reuse the original four-call, four-source-path, 56-emitted-line, 9-KiB response and
`t(P) + 2,400` response-token limits. The original packet ceiling of 42 necessary nonblank lines
and 3 KiB escaped source still applies, including the now-multiline imports. The task must succeed
without returning the whole large module. This scenario tests this useful non-preamble layout;
it does not establish exhaustive discovery of arbitrary dynamic imports.

## K: two independent binding edits surround retained context

The entrypoint calls an unchanged quota-policy function. One module import above that function
binds a percentage helper; another below the function binds a warning-threshold helper. Both
imports execute before a request. At HEAD the helpers come from one file: integer floor percentage
and threshold 90. Between phases only the two import lines change to another file providing
integer ceiling percentage and threshold 91. The middle policy body remains byte-for-byte equal.
The blocked boundary remains 100 percent. Capacity is positive; use 1,000 in literal probes.

| Used bytes | HEAD status | Current status |
| --- | --- | --- |
| 895 | clear | clear |
| 900 | warning | clear |
| 905 | warning | warning |
| 1000 | blocked | blocked |

These examples distinguish partial repairs: changing only the rounding binding makes 895 warn;
changing only the threshold binding leaves 905 clear. Fixture probes must establish those facts
independently of RepoScout. The two changed imports must appear in separate public change ranges.

Initial packet P1 needs the entrypoint/binding, unchanged policy body, both active imports and both
old helper definitions. It remains at most three paths, 48 lines and 2 KiB escaped source. Delta
packet P2 needs both new import lines and both new helper definitions: at most two paths, 24 lines
and 2 KiB escaped source. Freeze both packets before any CLI call.

Reuse the existing binding-variant budgets: initial four calls / 8 KiB responses / 64 source lines /
`t(P1) + 2,000` tokens; follow-up four calls / 8 KiB / 32 source lines / `t(P2) + 2,400` tokens.
All initial responses and stale-hash attempts remain in the complete eight-call episode cost.

Every necessary fresh chunk must contribute evidence; selecting only the first complete result
is insufficient. Retain unchanged middle source with its original hash/snapshot/span and separate
complete public change proof. Never relabel old bytes with the new identity. Current reads must
deliver both binding changes and their actual helpers without re-emitting unchanged policy-body
lines or the gap between disjoint selected ranges. No fixture/oracle path or private span may
guide driver requests.

## Reporting

Run each new case on the unchanged public driver first and preserve its actual unmet obligations.
Adapt the driver only after observing that result. If the public CLI itself fails, record and fix
the product failure separately; do not label a driver limitation as a product bug. Preserve every
older case, budget and negative control. Report measurements and remaining scope limits alongside
the [existing results](user-efficiency-results.md).
