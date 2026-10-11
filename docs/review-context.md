# Prepare a pull-request review

← [Documentation index](README.md)

`review-context` prepares revision-pinned evidence for a human or agent reviewing a PR: changes,
changed declarations, potential impact, test hints and source-token costs. It does not review the
correctness of the patch, call a model, allocate agents or split work. The caller makes those
decisions. Tokenization always uses the existing default, configuration or explicit `--encoding`.

```sh
# Compare two commits directly; HEAD is the default head
reposcout review-context . --base origin/main --head HEAD -f json

# Use the PR branch's unique merge base explicitly
reposcout review-context . --base origin/main --merge-base -f json

# Select an initial reading list; source remains opt-in
reposcout review-context . --base origin/main --context \
  --context-budget 24000 --context-max-files 20 -f json

# Include selected whole-file source and unified patches in one output allowance
reposcout review-context . --base origin/main --source --diff \
  --budget 16000 --max-output-bytes 131072 -f json

# Compact agent brief: rewrites, moved code, remaining hunks and direct consumers (at most 8,000 tokens)
reposcout review-context . --base origin/main --head HEAD --brief
```

References must already exist locally; the command does not fetch, checkout or contact a PR
provider. It requires a Git worktree on Unix and UTF-8 changed paths. `PATH` must be a directory; a subdirectory restricts
the changed paths that seed the review, while importers elsewhere in the repository remain
eligible. Use [changes](source-queries.md) for worktree/index comparisons and existing
`--review=deep` for finding-level regressions.

## Identity and evidence

Both references resolve once to commits and trees. Direct comparison is the default. With
`--merge-base`, exactly one common base is required; requested and actual base commits are both
reported. Sources, hunks, declaration mappings, hashes and resolver configuration come from the
same immutable trees. Dirty source files, staged edits and live resolver configuration do not
enter either side. Current RepoScout configuration and ignore policy still govern eligibility.

The change inventory includes additions, deletions, renames, mode and type changes, hidden paths,
unsupported formats, binaries, symlinks and submodules. Inclusion in this inventory does not
grant permission to read excluded content. Each side reports its path, mode, object ID,
availability and captured SHA-256 where available. Symlinks and submodules are never followed.
Unavailable or unsupported declaration analysis retains raw change evidence when text capture
succeeds. Mode-only changes have no invented changed declarations.

`hunk_status` distinguishes available, partial and unavailable change analysis. When either
side's content was not captured (for example because of a size or input budget), `hunks` is `null`;
a numeric zero means the comparison was performed and found no text hunks. Added/deleted files
compare the existing side against empty text. Interpret `hunks_omitted` only when `hunks` is a number:
it counts discarded computed hunks and remains zero when analysis could not run.
Each side's `mapping_status` separately qualifies its changed ranges and declarations,
independently of source `extraction` status.
`totals.changes_without_hunks` counts retained change pairs without computable hunks,
including pairs for which only one side fit the shared capture budget.

Pure insertions or deletions can leave one side without changed lines. For existing declarations,
the query maps unchanged header-start lines across the captured hunks and requires matching names
and kinds before retaining the opposite declaration. `counterpart_definitions` counts these
additions; `unprocessed_counterparts` reports bounded mapping work that could not finish.
`ambiguous_counterparts` counts ambiguous matches, which retain partial mapping status.
`counterpart_seed_mapping_incomplete` marks a partial or unavailable mapping on the supplying
side; its unknown missing declarations are not counted as finished or as counterpart-work omissions.
The receiving side retains partial mapping and conservative file-based reference evidence.
A newly added/deleted header-start line or absent file side
does not acquire a guessed counterpart. `ranges` continues to describe actual changed lines.

Each revision has its own graph. File-import dependents include direct and transitive reverse
edges; changed files' direct dependencies are also candidates. Relations retain direction and
resolver provenance. Concrete symbol-reference evidence is separate from file-import impact and
uses the existing conservative Rust and JS/TS/TSX resolvers. These are potential static effects, not a
promise that every consumer breaks or that runtime consumers have been found. A deleted file can
still expose its old importers in the base graph.

Symbol references expose `change_basis`: `changed-definition` means a mapped changed declaration
touches the reference. When declaration mapping is partial or unavailable on a captured changed file,
`changed-file` preserves its resolved references without claiming that their declarations changed.
These candidates use `file-reference-source` / `file-reference-target` roles; precise mappings use
`concrete-reference-source` / `concrete-reference-target`. A mapped changed declaration takes
precedence when both bases apply. Mapping status and the actually mapped declarations remain
unchanged; file-based evidence never invents a changed declaration.

Resolver-configuration scope is a directory-based hint, not a proven call relationship. Test
evidence distinguishes filename conventions and Rust inline-test syntax; it is neither an
executed test result nor measured coverage. Existing graph and language limitations apply.
Changed root resolver configurations can therefore suggest files throughout the repository.
Unchanged resolver configurations support resolution but are not automatically added to the
reading list: being read by a resolver is not evidence of relevance to this change.
The shared graph's path convention cannot represent a literal Unix backslash unambiguously.
Such paths retain change, source and cost evidence but are excluded from graph inputs and counted
in `coverage[].unsupported_graph_paths`; they can never alias a slash-separated path.

## Token costs and optional reading list

`totals.candidate_tokens` counts complete captured candidate files once per revision/path.
Base and head are intentionally separate: a reviewer may need both. Unknown sizes are `null`
and counted separately, never silently treated as measured zero. Counts use the configured
tokenizer; there is no model detection or tokenizer switching.

`totals.diff_tokens` is the separate cost of captured unified patches. Adding it to source costs
describes reading both representations, including repeated text. These figures exclude harness
instructions, conversation history and model output. They are not a forecast of total agent
usage. The rendered response has its own hard token and byte limits.

`--context`, `--context-budget`, `--context-max-files` or `--source` enables a deterministic initial
reading list. It prioritizes changed sides, test candidates, direct neighbors and then more
distant candidates; ties use distance, path and side. A whole file must fit both the remaining
source-token allowance and file-count allowance. Every candidate retains a selection reason.
The defaults come from `[context]` configuration (32,000 tokens and 25 file sides unless changed).
CLI overrides keep the ordinary execution-profile and absolute safety limits.

Without these flags there is no selection. `--source` delivers complete selected files or omits
their bodies explicitly; it never cuts source mid-file. The source selection budget does not
grant extra output capacity. Selection totals describe the initial list before output projection.
Further definition reads can use `read --snapshot <reported-tree>` with the reported path/hash.

## Agent brief

`--brief` is an opt-in text projection of the same pinned facts for an agent's first read. It
renders Table (the default for `--brief`, also when stdout is not a terminal) or one fenced
Markdown block; JSON and NDJSON are rejected, and it cannot be combined with `--context`,
`--context-budget`, `--context-max-files` or `--source`. Unified diffs are included without
`--diff`. Without `--budget` the brief renders in full up to 8,000 tokens and is projected to
8,000 tokens beyond that; an explicit `--budget` always sets the limit, and `--max-output-bytes`
applies as usual. The brief is not cut to the 100-entry output limit; its own projection applies.
The same input renders byte-identical output.

- The header names short base/head commits, comparison mode and totals. Trees, blobs, hashes and
  modes are not repeated; read files with `git show <commit>:<path>`. A `gaps:` line lists only
  nonzero gaps that limit evidence about this change with exact base/head counts: changes not
  analyzed or without hunks, incomplete rename detection, inventory truncation, unavailable
  inputs, changed files without graph, resolver configuration errors, and changed files whose
  language has no call-reference resolver or whose call references are incomplete.
  Repository-wide counters such as unresolved references stay in JSON. A missing gap line means
  none were recorded; unknown hunk counts read `hunks unknown`.
- `rewrites` lists token edits that recur in at least three hunks, most frequent first, as
  `R1 \`old\` → \`new\``, `insert` or `delete`. Tokens come from the shared tokenizer; an edit
  inside one token is reduced to the differing part between word boundaries and marked
  `also inside names` for identifiers or `also inside literals` for strings and numbers. Only an
  identifier token replaced by an identifier token, whole or in part, is a rename. Keywords and
  value-like names (`true`, `null`, `and`, `not`, `Ok`, `Some`, `Self`, `globalThis`, `any`,
  `NaN`, …), numbers, strings, operators, punctuation, insertions, deletions and edits of several
  tokens are never renames, and a rename and another edit with the same text are separate
  rewrites.
- Hunks are displayed by what explains their edits. A hunk folds (rewrite-only or compact) only
  when every block has as many removed as added lines and each removed line becomes its paired
  added line byte for byte once the listed rewrites are applied to its tokens. Outside
  indentation-sensitive languages a compact line may also change its leading whitespace.
  Python, YAML, Shell, Markdown, Makefiles, Scala, Dockerfiles, unknown languages and every other
  language not known to ignore indentation are indentation-sensitive: there a hunk whose paired
  lines change leading whitespace is shown in full with its context lines, hides no line and
  carries no rewrite tag.
  - Rewrite-only: every edit is a listed rename proven pure at its site, that is, provably the
    rename of a plain binding; anything uncertain is compact. Both tokens must be plain
    identifier or type-identifier nodes in the syntax trees of the complete base and head files
    of a reviewed grammar (TypeScript, TSX, JavaScript, Python, Rust and Go; other languages and
    files without a grammar prove no rename), so text in strings, template literals, regular
    expressions, comments and parse errors never qualifies. Member, key, field, attribute and
    argument-name positions never qualify either: object keys and shorthand properties, member
    accesses, JSX attribute names, TypeScript parameter properties and export aliases, Python
    attributes, keyword arguments, keyword patterns, class-body attributes and methods and import
    module paths, Rust fields, field shorthands, variants, path segments, impl and trait members,
    labels, lifetimes, macro arguments and attributes, and Go fields, methods, composite-literal
    keys and qualified type names. The old name must be bound in the base file of its site and
    the new name in the head file, both in the same way: declared there in a non-import binding
    position (variable, function, class, parameter, pattern or the local alias of an import), or
    imported there from an in-tree module, that is, a TypeScript/JavaScript specifier starting
    with `.`, a relative Python import or an absolute Python module path naming a captured file,
    or a Rust `crate::`, `self::` or `super::` path. The original-name side of an import or
    re-export specifier (`import { user as u }`, `export { user as current } from`) is never a
    declaration and qualifies only through such an in-tree specifier. A name the file also binds
    outside the tree, or that code outside the tree spells, never qualifies: through an import
    from any other module, a TypeScript ambient declaration (`declare`, `declare global`,
    `declare module`), a Rust `extern` block, an item carrying `#[no_mangle]` or `#[export_name]`,
    a function with an `extern` ABI (`pub extern "C" fn`) or a `macro_rules!` macro, or a Python
    keyword-only parameter (after `*` or `*args`). Builtins, host globals and Go names
    declared in another file of the package are bound nowhere in the file and never qualify;
    neither do lowercase JSX element names, non-ASCII names or any name of a file with a non-ASCII
    identifier. Within each file the applied renames must be one-to-one: a site whose old name
    also becomes another new name in its file, or whose new name also replaces another old name
    there, is not pure. The new full identifier must occur as a whole word in no blob
    of the base tree and the old one in no blob of the head tree, files without an inventory and
    binaries included; a tree whose blobs exceed 64 MiB, a scan past the review deadline and any
    identifier beyond the first 4,096 distinct ones checked prove no rename. Spelling changes with
    language meaning are never pure: a Go rename that changes the case of the first letter, a
    Python rename that adds or removes a leading underscore or involves a dunder name, and in
    test files a rename that adds or removes a `test` prefix or suffix. Rename folding is
    syntactic and textual, not semantic, and moves are not checked for rebinding through imports.
    Such hunks are not printed, but listed under
    `rewrite-only hunks:` by rewrite combination as compressed `path:line` sites; paths with
    whitespace or control characters are shell-quoted like the fetch commands
    (`'src/sp ace.ts':3`, `$'src/tab\tx.ts':3`). When the budget allows, a few `e.g.` examples
    cover every rename with its changed lines.
  - Compact: every edit is a listed rewrite, but at least one is not a rename proven pure or a
    line also changes its indentation. Such hunks print only their changed lines, each run of
    consecutive lines starting with its line number, without context lines, under a header
    tagged with their rewrites, such as `[R1 R2]`.
  - Full: every other hunk, printed with its context lines unless some of its lines are hidden
    as unchanged or moved. Its header names the rewrites it contains and, for an edit that
    shares a side or all but one token with a rewrite or has its text in the other class,
    `deviates from Rn`.
- A changed line is hidden when it recurs in another hunk once old spellings of single-token
  renames are replaced by the new ones in identifier tokens of removed lines; a reversed rename
  never matches. Lines never pair within their own hunk. A recurring line counts inside a run of
  at least three such lines (one other line may interrupt the run) or when it is unique on both
  sides and at least eight tokens long; blank lines count only between hidden lines. Recurring
  lines count as moved only as whole declarations: every non-blank line of a removed base
  declaration pairs, in order, with the same indentation and with the same line count, with a
  line of an added head declaration. Statements moved within or between declarations, partly
  moved, edited or re-indented declarations and declarations moved between a test file and a
  non-test file, in either direction, stay shown. A moved declaration whose lines differ by
  renames counts as moved only when each line becomes its counterpart byte for byte and every
  rename in it is proven pure as above; those lines count toward the hunks of their rewrites and
  are listed under `moved lines with rewrites (head lines):` by rewrite and head path. A
  declaration moves only when it keeps its free names: every top-level statement of the target
  head file outside the folded moved declarations that declares a name the moved lines use, imports
  aside, must also be a byte-identical top-level statement of the source base file, and the
  headers of its enclosing classes, impls and other containers (their text before the body:
  name, bases, extended class or implemented trait) must be byte-identical in source and target,
  so a top-level declaration moves only to the top level; otherwise the move is shown. A line
  that becomes its counterpart at the same position of its change block byte for byte after pure
  renames is hidden as unchanged.
  `moved hunks` lists hunks whose every changed line recurs, as
  `base path:lines → head path:lines` (`:lines` within the same file). Full hunks with hidden
  lines print only the lines that are not hidden, numbered where a run of consecutive lines
  starts, and their header counts the lines hidden as unchanged or moved, with up to three
  destinations or sources. Token diffs work on blocks of at most 2,000 tokens; line matching
  uses hashed line keys, linear passes and one sort, and diffs above 200,000 changed lines skip
  it and print every line, all under the review deadline. Added, deleted and modified files
  whose hunks moved keep a change entry; when none of their hunks is shown, the entry lists each
  moved hunk header with the place its lines moved to or from.
- `renamed or mode changed without a shown hunk:` lists renamed and mode-changed files whose
  hunks are all rewrite-only or moved (at most 20, then `+N more: git diff -M --summary <base>
  <head>`), with octal modes and the
  folded hunk counts, so a rename or permission change is never hidden.
- `old spellings still in head (verify intent):` lists distinctive old spellings of renamed
  identifiers or quoted literals that head no longer declares, whose line count drops but stays
  above zero, with base/head line counts; a name whose head line count does not drop, for
  example because new uses replace removed ones, is not listed. Plain words in Markdown and text
  files count only when quoted. At most twelve lines are shown across all names, then
  `… +N more lines` with one `git grep` command that lists them all.
- Each remaining change lists its status and path, hunk counts by role, changed definitions
  paired by kind and name with line ranges per revision, and the hunks still shown.
- Impact lists only incoming distance-1 evidence. Concrete call/reference sites are grouped as
  `kind -> target: path:line source, …`; sites inside changed lines, outgoing references and the
  change's own imports are visible in the diff and not repeated. `imported by:` lists at most six
  importers, those without a listed reference first, then an exact `+N more`. `[test]` marks
  filename/syntax test hints. Evidence identical in both revisions renders once; single-revision
  evidence is marked `(base only)` or `(head only)`. Transitive dependents, resolver-configuration
  scope and relations of changes without listed details are counted, not listed; at most one
  shortest `entry:` chain per changed file leads from a non-test head file without non-test
  importers through its intermediate importers.
- For direct importers without a resolved call/reference to the changed file (for example Python,
  which has no call-reference resolver), `usage lines` list importer lines that name identifiers
  of that change as `path:line  text`. Terms are the change's callable declarations plus callable
  declarations of the changed file whose spans use them, and names bound by `TERM as ALIAS` on the
  importer's import lines. Code matches whole words case-sensitively; quoted literals that are
  not declared identifiers and are a single lowercase word match only inside quotes. Import
  statements count only when an importer has no other matching line. Lines come from the pinned
  head snapshot, or from base (marked `(base)`) for importers seen only in the base graph; lines
  inside a changed importer's own hunks, at a resolved reference site or listed as an old
  spelling are skipped. Each importer shows at most three lines, allocated round-robin across
  importers up to sixteen in total.
- Unchanged Markdown and text files in the pinned head tree that name changed definitions appear
  as `path:line text` (trimmed, at most three lines per file and twelve in total). Terms are
  changed definition names of at least two words; they match one identifier token in any case
  style (`eventIdentity`, `event_identity`, `EVENT_IDENTITY`), never the same words in prose.
  Working-tree and index content never contribute.

When the brief exceeds a budget, it keeps the header, the rewrite rules and the moved, rename,
stale and omission summaries, and admits the remaining items in a fixed order as long as both
budgets hold: rewrite-only sites, every change entry heading, shown hunks (deviating,
structural, line-for-line and deletion hunks of non-test files, then the same tiers of test
files, then compact hunks of non-test and then of test files), a change's
definition details right after its first admitted hunk, rewrite examples, remaining details,
importers, doc lines and usage lines. A bounded number of later details, examples, importer, doc
and usage lines that still fit are tried afterwards; entry headings and hunks are only admitted in
that order. Entry headings come first so that every change stays visible by path; hunks come
before details and examples because the diff is the evidence the other sections summarize. Doc
lines, importer lists and usage lines together take at most max(400, half the diff tokens). The
`omitted from output:` line counts every removal exactly, and change entries without a shown hunk
that do not fit are listed by path and status (at most 40, then `+N more`).

Omitted hunks are never dropped silently: `omitted hunks (N in M files):` lists them per file as
`path @@ +a,b +c,d` (`-a,b` for deletions), followed by one
`git diff --no-indent-heuristic <base> <head> -- <paths>` command per ten files. After forty files
a final line counts the remaining files and hunks and names the command without paths. Commands
use the 12-character resolved base and head commits (the merge base with `--merge-base`) and
shell-quoted `:(top,literal)path` pathspecs (`$'…'` quoting for paths with control characters),
so they print the same hunks from any directory of the repository and match every character of a
path literally.

Omitted compact hunks are not part of that per-file list. `omitted rule-explained hunks (N in M
files; every edit is a listed rewrite):` groups them by rewrite combination as
`R1 R2 (5 hunks): src/{a,b}.ts:12 lib/c.ts:4,30`, one compressed `path:line` site per hunk,
followed by one `git diff` command per ten files with the same flags and pathspecs. The sites
are admitted only once every hunk that is not compact is shown; otherwise each combination is
counted as `R1 R2: 5 hunks in 3 files`, followed by one command for all of them. Every omitted
compact hunk appears in exactly one group.

When the mandatory sections do not fit, they collapse level by level: first the stale-name list
becomes one count line, then each omitted-hunk list (per file and rule-explained) becomes one
line with its command, and finally rewrites, moves, renames, stale names and omissions (compact
hunks counted with all other hunks) become counts followed by
`full diff: git diff --no-indent-heuristic <base> <head>`. Each finer level, and finally the
compact-hunk sites, starts at a fixed budget: the smallest one at which it holds every item up to
and including the first shown hunk (the first change entry when no hunk is shown; for the sites,
every hunk that is not compact). From there it replaces the coarser levels once it admits at
least as many change entries and hunks as they admit just below that budget. A larger budget
therefore never drops a change entry or hunk shown at a smaller one and never uses a coarser
level. Only when even the counts exceed the budget does the brief fail with an error.

Hunks come from libgit2's Myers diff with three context lines and without Git's indent
heuristic; the emitted commands pass `--no-indent-heuristic` to match. A plain `git diff`, whose
indent heuristic is on by default, and any `diff.algorithm`, `diff.context`, `diff.renames` or
external diff setting can print other hunk boundaries for the same changes.

## Limits and omissions

| Bound | Default / ceiling |
|---|---|
| Rendered response | 4,096 tokens / 65,536 bytes |
| `--brief` rendered response | Full brief, at most 8,000 tokens without `--budget` |
| `--budget` | 256–65,536 tokens |
| `--max-output-bytes` | 1,024–1,048,576 bytes |
| Inventory | At most 10,000 paths per revision |
| Captured inputs | At most 10,000 file sides / 32 MiB shared by both revisions |
| Individual input | At most 8 MiB, additionally bounded by configured file/blob limits |
| Output entries | At most 100 changes, 100 candidates and 100 relations, then byte/token projection |

Configured input limits may narrow these ceilings. Changed sides are captured first;
remaining paths alternate between base and head. Rename detection is bounded separately and
reports when skipped. Its work limit applies to the repository-wide comparison before the
changed-path filter, preserving bounded detection of cross-boundary renames. Hunks and declaration
mappings reuse the existing bounded change mapper. The duration limit is cooperative: checks
between bounded analysis units and after graph construction, call resolution and rendering abort
an expired query with an error, including when the deadline expires during capture. Duration
exhaustion does not return a partial report. A single parser, Git or graph operation is not preempted.

Read `coverage` and omission counters before interpreting an empty result. Inventory truncation,
excluded/unavailable inputs, unsupported analysis, parse/configuration errors and unresolved
relationships can hide impact. Reported candidate costs describe the observed neighborhood, not
an upper bound on unknown consumers. `changes_not_analyzed` records change pairs omitted before capture separately from
`changes_omitted` (output projection). Other output omissions stay separate from capture gaps and
selection exclusions; totals survive output projection. Increase the output allowance for more
entries or use narrower changed-path scope. If even the status envelope does not fit, the command
fails rather than returning a misleading empty success.
Projection strategy 2 preserves compact change identities by reducing the largest declaration
lists, then range lists, before removing relations, context candidates or whole changes. Lists
retain deterministic prefixes and are reduced geometrically to bound projection work. Per-side
`definitions_omitted` and `ranges_omitted` count removed details; these output counts do not alter
`mapping_status` or analysis-gap counters. If whole changes still cannot fit, the largest compact
JSON entry is removed first. Omitted compact identities are then reconsidered against the actual
renderer and tokenizer before any details are restored; a byte-heavy but token-affordable identity
must not disappear merely because another path costs more tokens. The complete response must fit
both budgets in the selected format.
When bounded body-free output has lost concrete references touching changed declarations, those
references are reconsidered before weaker neighborhood metadata. A resolved test caller has the same priority
as any other resolved caller. Table and Markdown admit a reference together with any missing
revision/hash handles needed to read its endpoints; JSON and NDJSON already carry those identities
inside the reference. A packet that does not fit is rolled back completely, and smaller subsequent
packets are still tried. If none fits, existing type/import/file-based evidence remains available.
Already captured endpoint handles beyond the initial candidate prefix can participate, but the
final response still contains at most 100 candidates and 100 relations. This is output allocation
only: source-selection order, selected-file costs and analysis coverage do not change. Explicit
`--source` and `--diff` requests retain their established allocation, including affordable bodies.
Previously trimmed changes are reconsidered for complete detail restoration, smallest first,
so removing an oversized entry can make room for a small change's ranges and declarations again.
Remaining omitted relations and context metadata are reconsidered in their original evidence order;
requested diff and whole-source bodies follow them. Every admission is checked against both
actual output budgets, and all omission counts describe the final retained response.
Relations alternate between head and base so either side cannot consume the entire output limit
while the other has evidence; references touching changed definitions, type relations, file-based
references and imports follow that order within each side. File-based references prioritize
incoming cross-file references to incompletely mapped files, then outgoing references, then
same-file references.
Inventory truncation also includes unrepresentable unchanged path names. Unsupported inventory
counts refer to unrecognized formats, not just failed parsers; Markdown is recognized.

Table, Markdown, JSON and single-record NDJSON use the same facts and output budgets. Source and
diff text are absent by default. Table and Markdown render source and diff bodies with real
newlines and tabs, escaping other control characters; Markdown fences each body. Human output includes blob IDs, changed ranges, mapping status,
context distances and predecessors, and concrete symbol/type evidence; JSON and NDJSON retain the
complete structured records. `--output` and `--debug-log` must be outside the worktree and its
Git metadata (including linked-worktree common metadata) to
preserve the source and policy being reviewed. Analysis cache writes use the ordinary external,
best-effort cache; `--no-cache` disables them.
