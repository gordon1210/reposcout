Case I packet frozen before the first product invocation, 2026-10-06.

The literal spans in `fixture.rs` cover 25 nonblank lines across four paths:

| Fragment | Span | Purpose |
| --- | --- | --- |
| `src/invoices.py` | 1–2 | Establish the actual Invoice and discount alias bindings. |
| `src/models.py` | 1–9 | Establish the data declaration and cents/basis-point fields. |
| `src/invoices.py` | 5–8 | Connect the known payable definition to the alias and subtraction. |
| Active helper | 1–5 | Identify the actual rounding expression to change. |
| `tests/test_invoices.py` | 1–9 | Connect literal 904 and the three boundary examples to production. |

The helper-only repair changes `return round(amount * bps / 10000)` to
`return (amount * bps + 5000) // 10000`. The binding swap keeps that correct old
helper in place while importing a different faulty settlement helper. The
counterfeit test changes only its import to the correct demo implementation.
The noise fixture renames unrelated invoice handlers and adds a dashboard
selection handler; all necessary original packet spans remain byte-for-byte
unchanged.

An independent Python process imports production and checks literal payables
`[900, 905, 1005, 0]` before repair and `[900, 904, 1005, 0]` after repair. It also
executes the authored regression and records the actual helper module/name.
The counterfeit regression passes while the faulty production path still
returns 905. No probe output or packet path is available to the CLI driver.

The packet must stay within 42 necessary nonblank lines and 3 KiB JSON-escaped
source. Each episode keeps the original 4-call, 4-source-path, 56-source-line,
9-KiB response and `t(P) + 2,400` response-token limits. Both supported
tokenizers are explicit user inputs on the original representative. Timing
and RSS are recorded separately; neither changes evidence requirements.
