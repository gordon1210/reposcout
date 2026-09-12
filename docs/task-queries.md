# Search, definition plans and consumers

← [Documentation index](README.md)

Use the entry point already available to the task: `find` for an unknown declaration, `plan` for a
bounded set of known definitions and their supported environment, or `consumers` for proven call
and reference relationships. Known source can go directly to [read](source-queries.md). These
commands do not impose a scout → search → outline → read sequence.

The contracts below describe the locally implemented interfaces and limits. The
[38-run pilot](agent-evaluation.md) passed bounded answer checks but did not establish net
agent-token savings; see also the [evaluation procedure](../scripts/agent-eval/README.md).

## Find an unknown entry point

```sh
reposcout find 'retry delay duplicate payment' . -f json
reposcout find 'invoice total' src --match any --language Rust --kind function --limit 10 -f json
```

`find QUERY [PATH]` searches declaration names, paths, signatures, comments and bounded code.
`--match all` is the default: every query term must match somewhere in a candidate's fields.
`--match any` requires at least one term. Language and kind filters are case-insensitive exact
matches. Results group evidence by declaration and never include source bodies automatically.

Tokenization uses Unicode alphanumeric segments and Unicode lowercase, not full Unicode case
folding. It retains whole identifier segments and camel/acronym components; underscores and
punctuation separate segments. Query terms are deduplicated. This is lexical matching, not a
promise to understand a natural-language task semantically.

Ranking uses integer evidence weights: exact qualified name +1,000, exact simple name +900, exact
full path +700, exact filename/stem +600. Each matched query term contributes name 100, path 60,
signature 40, comment 20 and code 10 in its respective fields. Ties resolve by path, declaration
start byte, kind and name after descending score. Health risk, test presence and graph centrality
are not search bonuses; health exclusion does not exclude otherwise eligible navigation facts.

| Bound | Value |
|---|---:|
| Query length / distinct terms | 512 Unicode scalar values / 16 terms |
| Returned hit limit | Default 20; 1–100 |
| Lexically inspected declarations per file | 2,048 |
| Terms per field / term length | 128 / 128 Unicode scalar values |
| Code / comment bytes per declaration | 4,096 / 2,048 |
| Comment nodes / syntax nodes per file | 4,096 / 100,000 |

The body-free report separates `coverage` from `limit_omitted` and `budget_omitted`. Coverage
reports inspected, unsupported, unavailable, parse-error and field-truncated files, declaration
counts and truncated fields. No matches in the inspected prefix do not prove absence elsewhere.

Each hit includes declaration identity, captured hash and snapshot, score, matched fields, reasons
and a structured `read` target. When following it, preserve the expected hash:

```sh
reposcout read . --symbol src/payments.rs retry_delay \
  --expect-hash src/payments.rs '<SHA256_FROM_HIT>' -f json
```

A changed file fails the hash expectation instead of silently applying stale selection evidence.
Search relevance and unique identity are different: multiple same-named definitions can still
require disambiguation by the source reader. Existing `locate` name matching remains unchanged.

## Plan definitions under separate source and output budgets

```sh
reposcout plan . \
  --symbol src/billing.rs invoice_total --line src/inventory.rs 12 \
  --context-budget 12000 --budget 4096 -f json

reposcout plan . --snapshot index --symbol src/billing.rs invoice_total --source -f json
reposcout plan . --file src/billing.rs --max-definitions 8 -f json
```

`--symbol FILE SYMBOL`, `--line FILE LINE` and `--file FILE` supply explicit seeds. A file seed
considers its declarations; it is not a request to return the entire file. Snapshot and hash
expectations follow the [source-query identity contract](source-queries.md#choose-the-source-snapshot).
Unseeded planning is permitted for the worktree; index or revision planning requires explicit
selectors. Without a seed, expansion is not presented as direct task evidence.

| Option | Default | Range |
|---|---:|---:|
| `--context-budget` | 12,000 source tokens | 1–65,536 |
| `--max-plan-files` | 8 | 1–32 |
| `--max-definitions` | 16 | 1–32 |

The planner is a pure consumer of captured declaration costs and environment facts. It counts
selected source ranges after overlap deduplication. `selected_tokens` records that source cost
before output projection; it is not the size of the rendered response. Oversized, ambiguous,
unresolved and unavailable seeds stay distinguishable. Discovery gaps, uncosted declarations,
planning omissions and output omissions are separate quantities.

The first supported environment is a uniquely identified local type used in a function or method
signature in Rust, TypeScript or TSX. Environment extraction is bounded to eight additions and
4,096 inspected signature nodes; costing retains at most 4,096 definitions per file. Ambiguous or
unsupported type bindings remain gaps. Body dependencies, arbitrary imported types, runtime
receivers and unbounded transitive dependencies are not silently filled in. A syntactically
complete function can still require further task context.

Default output is body-free `DefinitionPlanReport`. `--source` explicitly includes an optional
`source` block using the shared complete-definition reader. The **combined plan and source** must
fit the rendered token/byte budget. The separate context budget does not grant an additional
output allowance. Source chunks preserve captured identity and deduplicate overlaps; output
omissions do not retroactively change the source cost originally selected by the planner.

Definition planning and source delivery use the existing Unix-only capture boundary. Ordinary
whole-file `--context` planning retains its own established interface.

## Inspect conservative consumers

```sh
reposcout consumers . --symbol src/billing.rs invoice_total -f json
reposcout consumers . --line src/billing.rs 12 \
  --direction incoming --depth 2 --limit 20 --path-limit 20 -f json
```

Consumer queries use worktree facts. They require known symbol/line seeds and optionally their
expected hashes. Invalid, missing, ambiguous or stale seeds fail before graph output. There is no
consumer `--source` or `--snapshot`: follow the returned exact qualified-symbol/hash target with `read` when
source is needed.

A diff enters this workflow through the existing changed-definition query:

```sh
reposcout changes . --working -f json
# Use a selected current-side definition and its file SHA-256 from that result.
reposcout consumers . --symbol src/billing.rs invoice_total \
  --expect-hash src/billing.rs '<CURRENT_SIDE_SHA256>' -f json
```

Keep the content side explicit. A staged/index or historical result is not automatically a valid
worktree seed; its hash must match the captured worktree or the query fails. Historical consumer
topology is unsupported in this interface. Old definitions remain readable through the snapshot
source reader even when no current consumer seed exists. There is no automatic requirement to
query every changed definition.

Direction is `incoming` by default, or `outgoing`/`both`. Depth defaults to 1 and is capped at 8;
result and distinct-path limits each default to 20 and are capped at 100. At most 32 seed targets
are accepted. Traversal deduplicates reachable symbols and reports shortest-path evidence;
depth omissions count reachable symbols beyond the requested depth, not every possible path.
Cycles do not justify repeated whole-graph output.

| Language | Proven initial binding forms | Remains unresolved |
|---|---|---|
| Rust | Unique local direct targets, imported aliases, statically resolved `crate`/`self`/`super`/module-qualified targets | Receivers, trait/dynamic dispatch, function-valued/computed targets, macros and glob imports |
| JavaScript, TypeScript, TSX | Unique local targets, named/default imports and namespace-import members | Ordinary object receivers, optional/computed/member-chain targets, dynamic imports and `require` binding |
| Other languages | No equivalent call-binding support is advertised | Explicit unsupported coverage |

Parameters and local bindings shadow candidates. Default exports require an associated unique
exported declaration; duplicate targets and TypeScript overloads remain ambiguous. A matching name
in another file never establishes a relationship without module/import evidence. Call and
non-call reference kinds remain separate from file-import and type-inheritance edges.

Each resolved relation carries a captured site, source/target identity and binding provenance such
as `local-lexical`, JavaScript import rules or Rust qualification rules, plus existing module
resolver evidence. A file whose fact or work limits were reached cannot promote retained relations
to safe bindings: missing shadow/import facts could invalidate them. Those relations remain
unresolved with the limit reason. Inspect extraction and resolution coverage before interpreting
zero consumers; result and output omissions are separate again.

## Shared output and automation rules

`find`, `plan` and `consumers` support table, JSON, Markdown and NDJSON. Captured stdout defaults to
JSON; terminal output defaults to table. Each complete rendered response, including metadata,
formatting and final newline, fits both `--budget` (default 4,096; 256–65,536 tokens) and
`--max-output-bytes` (default 65,536; 1,024–1,048,576 bytes). A minimum status envelope that cannot
fit is an error, not malformed or silently clipped output. Pretty JSON costs count too.

Capabilities expose `find_query`, `definition_plan`, `call_query` and `task_diagnostics`; inspect
those fields when compatibility is uncertain. Existing compact scouting stays body-free and
bounded. Normal source reads and lexical search do not require call topology. Cache facts use
analyzer 20; the additive report schema stays 2.0. Local cache reuse and short tool output alone do
not establish a reduction in total model tokens.
