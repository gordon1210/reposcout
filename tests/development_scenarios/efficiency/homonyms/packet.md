# H frozen evidence and controls

Frozen before any RepoScout response is observed. This case uses two active Python packages;
each exposes `POST /discount`, imports `discount_cents` at module scope and binds the request
handler in its own `ROUTES` table. Staff preview is an active application, not an unused decoy.

The independent one-shot Python probe dispatches literal `(WELCOME,1999)`, `(WELCOME,2000)` and
`(WELCOME-X,2000)` requests. Storefront must return `[0,500,0]` at both revisions. Staff returns
`[500,500,0]` at base and `[500,500,500]` at head. Responses also carry the literal application
identity. The probe records real route, policy and test import identities and executes the actual
request checks; unchanged staff checks catch the intentionally expanded prefix at head.

Necessary source is frozen by `World::packet`: both active API bindings, handler calls and dispatch
tables; staff rule before and after; storefront's exact code and `>=2000` threshold; literal request
checks imported from each package's dispatcher. Unchanged API source can be delivered once only if
public pinned identity evidence proves its identical bytes at the other revision. Storefront's
head API binding must additionally be delivered in the real coupling control. The unrelated
health handler, demo rule/API and application settings are outside the necessary packet.

Every episode uses at most six public CLI calls, 24 KiB complete stdout/stderr, 7,500 response tokens,
six delivered source paths, 110 nonblank source occurrences and 2,000 source tokens. Every response,
signature, failed call and retry counts. The fixture is bounded to 20 files / 64 KiB. The primary
independence case uses explicit `o200k_base` and `cl100k_base`; the two controls use `o200k_base`.

The real coupling control changes the storefront API's production import to `staff.rules`; real
storefront responses become `[500,500,500]`, and the original storefront request check fails.
The counterfeit changes only the storefront test to call the staff helper directly. Its passing
assertions do not establish a storefront request contract; real storefront responses remain
`[0,500,0]`. A missing genuine head request-check obligation must be reported separately from
successful production-route evidence and budget accounting.
The negative packet includes the actual helper-only test so its binding is inspected and rejected
by the same genuine request predicate, rather than accepting an absent test as a successful control.

The driver gets the public task's application names, discount rule, base/head and CLI handles only.
The fixture's literal request method and URI are private truth, discovered from returned evidence.
It never receives this packet, internal paths, probe results or private fixture access. The oracle
may inspect independently authored source and checks every captured hash, pinned tree and span.
