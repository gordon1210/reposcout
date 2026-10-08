# Application checks

The check suite exercises the public dispatch interface and a few format-specific helpers.
Each module exports a function; `tests/run.mjs` loads and runs them in sorted order within one
Node process. A failed assertion is printed with its stack and contributes to a nonzero final
exit code. There is no fixture installation, package download, test daemon, real clock, or live
external account.

`tests/support.mjs` provides fresh workspaces, expected-success and expected-failure assertions,
ordinary document creation, review, release, and export helpers. These helpers call the public
actions and do not bypass authorization. Test data is literal small user content rather than
generated volumes. State snapshots verify atomic rollback and detached responses.

Checks cover collection hierarchy and memberships, immutable document history and concurrent
edits, review independence and comment blockers, publication identity, export formats and job
lifecycle, search relevance and visibility, preview behavior, shares, labels, watches, retention,
templates, imports, links, outlines, administration, and audit. Passing checks establish these
specific assertions, not exhaustive correctness for every combination of edits and publication
events. Review conclusions may require targeted additional evidence.

When reporting validation, name only commands actually run. A checked-in filename alone does
not establish execution or a passing result. Static source analysis can also substantiate a
review if the active path, contract, concrete trigger, and effect are correctly connected.
