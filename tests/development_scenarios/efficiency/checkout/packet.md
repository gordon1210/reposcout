# Frozen checkout wire-compatibility packet

Authored before any product response is inspected. The user supplies only repository,
base/head revisions, `POST /checkout`, and the integer-cent/EUR compatibility rule.

Requests are `{"items":[{"unit_price_cents":1250,"quantity":2}]}` and
`{"items":[{"unit_price_cents":99,"quantity":1}]}`. Base and repaired responses are
literally `{"amount_cents":2500,"currency":"EUR"}` and
`{"amount_cents":99,"currency":"EUR"}`. Faulty head responses are literally
`{"amount_cents":25,"currency":"EUR"}` and `{"amount_cents":0,"currency":"EUR"}`.
Both calculator results remain `[2500,99]`. The genuine dispatcher test passes at
base/repair and fails at faulty head. The calculator-only counterfeit passes at
faulty head but cannot establish request-contract coverage.

The sufficient packet is the complete natural route module (registration, imported
handler, dispatch), handler module (calculator and serializer bindings), calculator,
base and head serializer, and genuine request test. These occupy five source paths;
both serializer occurrences count. Imports stay at module scope. Every piece proves
either active reachability, units, wire behavior, or an applicable literal assertion.
The packet is costed before the first CLI call for the explicit encoding being used.

Each independent episode permits six calls, 24 KiB raw stdout/stderr, 7,500 response
tokens, five delivered source paths, 110 nonblank source-line occurrences and 2,000
source tokens. Metadata, signatures, patches, errors, retries and repeated source all
count. The quiet/noisy pair shares these caps; noisy raw responses may exceed quiet
by at most 4 KiB. No output is filtered before accounting.

Noise is unrelated telemetry CSV reporting plus a generated dashboard JSON snapshot.
The source rewrite is formatting only; the snapshot changes JSON presentation only.
A separate one-shot fixture probe requires equal literal CSV results and equal JSON
values. The large snapshot is realistic telemetry data, not source or parser padding.
Neither noise body belongs to the required checkout packet. Fixture construction
enforces the pre-agreed 32-file / 128-KiB ceiling.

Minimal variants: genuine quiet/noisy faulty review, genuine repaired review, and
calculator-only counterfeit review. The counterfeit is a separately named negative
control; it cannot be reported as completing the positive compatibility task.
The independently frozen repair episode compares faulty base (25/0 cents and failing
request checks) with corrected head (2,500/99 cents and passing request checks), under
the same caps. Its old/new serializer packet reverses the two literal bodies.
No whole-PR safety verdict, model bill or latency improvement is implied.
