# Review policies and publication preparation

Governance is opt-in per collection. With no applicable assignment, existing review and publication actions retain their behavior. Policies do not rewrite stored releases. A release is still an immutable selection of approved revision identities; a preparation request is a separate request to choose candidates for a **new** release.

## Policies and inheritance

`governance.policies.create` accepts `{definition}`. A definition contains a name, optional description, rules, and review stages. `governance.policies.revise` additionally requires `policyId` and `expectedVersionId`; it creates another immutable version. Existing assignments remain pinned to the old version. `governance.policies.get` exposes version history and the resolved rule set to workspace managers.

Each rule has a stable lowercase `id`, user-facing `message`, `severity` (`error` by default or `warning`), optional `when` expression, and mandatory `require` expression. Rules with a false `when` are skipped. Failed warnings appear in the assessment but do not prevent submission.

A definition can pin `extendsVersionId`. Its rules replace inherited rules having the same ID, preserving deterministic order; new IDs append. `removeRules` explicitly deletes inherited IDs and rejects unknown IDs. An omitted `stages` inherits the parent's stages; a supplied nonempty array replaces all stages. Resolution rejects cycles, more than sixteen ancestors, and more than 64 effective rules.

`governance.policies.assign` takes `collectionId`, `versionId`, and, when replacing an assignment, `expectedAssignmentId`. Collections inherit the nearest ancestor's assignment. A local assignment overrides that ancestor. Setting `versionId: null` removes only the local assignment and restores inheritance. Changing a collection's location naturally changes inherited requirements. `governance.policies.effective` returns assignment provenance, ancestry traversed, version lineage, and policy fingerprint.

Policy management requires workspace manage permission; assignment requires collection manage permission. Reading effective policy requires collection read permission.

## Expression contract

Expressions are data, never executable JavaScript or regular expressions. Unknown fields, facts, operators, and incompatible value types are rejected. Every expression is bounded to depth eight, 128 nodes, and sixteen logical operands per node. Lists of alternatives contain at most 32 values. String operands contain at most 500 characters.

Supported forms:

```json
{"op":"all","args":[{"op":"gte","fact":"words","value":20},{"op":"eq","fact":"unresolvedMarkers","value":0}]}
```

`all` and `any` evaluate every operand to preserve a complete explanation. `not` has one `arg`. Comparisons use `fact` and `value`: `eq`, `ne`, numeric `gt/gte/lt/lte`, `contains` for a string substring or exact list member, and `oneOf` for bounded scalar alternatives. All comparisons are case-sensitive; there is no coercion.

Facts include title, language, authorId, tags, source character and line counts, prose word count, section titles/count, empty sections, inline Markdown link count, unresolved editorial markers (`TODO`, `FIXME`, `TBD`), fenced code block count, and an unclosed-fence flag. The bounded Markdown-oriented extractor handles ATX headings and matching backtick/tilde fences; it is not a full Markdown renderer. Code does not count as prose or editorial markers. A code block counts as content for its immediately preceding section. Heading text is excluded from prose word counts. An empty section has no nonblank prose or code before its next heading. The shared body size limit is inherited from document revision creation.

## Assessments and selected sources

`governance.assessments.preview` evaluates `{documentId, revisionId?}` without writing. Omitted revision selects the current working revision once at request time. `governance.assessments.create` persists the same facts and complete expression explanations, deduplicated by source digest, policy fingerprint, and collection. Each assessment records both the revision ID and a digest covering title, body, tags, language, and author identity. Equal body checksums do not make different metadata or revision IDs interchangeable.

`governance.assessments.get` returns the original immutable assessment and separate booleans indicating source availability and current policy/collection/source agreement. It never recalculates the historical assessment from working text. If retention removes an unreviewed revision, the evidence remains readable while its availability flag becomes false.

`governance.plans.create` accepts `{documentId, revisionId?, assessmentId?}`. It requires applicable policy and a passing assessment of exactly the selected source under the current collection and policy. It records the full resolved policy and source identity. Subsequent working edits do not change that plan. Source or collection mismatch and changed effective policy require a new plan. Merely creating a new policy version does not invalidate a plan until an assignment changes.

## Staged review and evidence

A policy has one to eight ordered stages. Each has `id`, `name`, optional `distinctReviewer`, and up to 24 checklist items. Checklist items have `id`, `label`, and optional `evidenceRequired`.

`governance.submit` takes `{planId, assigneeId, note?}` and opens the next stage through the existing real review service. This preserves independent-reviewer checks, permissions, open-review uniqueness, comments, and approval creation. The existing `reviews.open` action also accepts `planId` for governed documents; without it a configured document cannot bypass planning. Ungoverned documents still use their original action semantics.

`governance.checklist.record` takes `{planId, stageId, checklistId, completed, evidence?, expectedEntryId?}`. Only the assigned reviewer of an open stage may record evidence. A completed evidence-required item needs nonblank text. Every update appends an immutable record with a source identity, policy fingerprint, ordinal and superseded entry ID. Optimistic concurrency prevents overwriting another recording. Reassignment invalidates the old reviewer's checklist completion until the new assignee records their own evidence.

`reviews.decide` retains the existing comment and assignment checks. Approval additionally requires complete prior stages, all current checklist items, current policy, and any distinct-reviewer requirement. Rejection remains possible after a policy change so stale open reviews can be closed. A rejected plan cannot advance; create a fresh plan to retry. Completed checklist entries cannot be changed retroactively. Intermediate approvals exist for audit but cannot publish a governed document.

## New-release readiness

`governance.publication.prepare` accepts `{documentIds}` for one to fifty unique documents, requiring publish permission for each. It returns a deterministic per-document list of approval candidates, source identities, policy version/fingerprint, plan and assessment IDs, review stages, checklist evidence IDs, reviewer authorization evidence, and exclusion reasons. At most 1,000 approvals per document are inspected; larger histories return `planning_limit` rather than a partial result.

The planner selects the newest **eligible approved revision**, ordered by revision sequence with approval ID as a deterministic tie-break. It can select an older revision when a newer working revision is unapproved or a newer approval has not completed the governance stages. `workingRevisionId` remains separately visible. `entries` contains directly usable approval references; `ready` is true only when every requested document has a candidate. Preparation is read-only and creates no release or approval.

`releases.publish` rechecks selected governed approvals at execution time: source identity, active policy, completed stages, final-stage approval, checklist records, independent reviewers, and current reviewer permissions. Thus a prepared candidate can become ineligible before publication. Existing releases remain readable from their captured selection even when policies, working documents, or reviewer permissions subsequently change. Ungoverned publication retains existing behavior.

## Operational limits

This module relies on the application's transaction boundary for atomic writes and on its cloned dispatch output for isolation. It does not add background jobs or dependencies. Retention can remove a planned source before an actual review protects it; later submission fails explicitly and never substitutes working text. Saved assessment metadata is intentionally retained. Policy editing, assignment changes and workflow events use the existing audit stream.

## Policy evolution and assignment impact

`governance.policies.compare` compares two immutable version IDs. It reports added, removed, reordered, and field-changed rules, stages, and checklist items. Semantically identical rules under a new version can have unchanged semantics but changed identity: existing assessments are deliberately tied to the exact version and inheritance lineage they evaluated. Rule origin provenance does not masquerade as an expression change.

`governance.assignments.preview` forecasts a local assignment replacement or removal without mutating state. Inputs are `collectionId`, proposed `versionId` (or null to restore inheritance), `expectedAssignmentId`, and `expectedVersionId` naming the target policy's latest version. The proposal can intentionally pin an older version, but only with acknowledgment of the policy's current head. The preview walks descendants, respecting nearer local assignments; these shield their documents from the parent change. For each inspected active document it evaluates current working content under proposed requirements and classifies historical assessments, plans, and approvals as usable or unusable under current and proposed policy. It reports exact record IDs, revision identities and reasons such as changed policy, missing source, incomplete stages, or lost reviewer authorization.

The preview inspects at most `documentLimit` documents (default 100, maximum 500) and `recordLimit` historical records across them (default 500, maximum 2,000). `omittedDocuments`, `omittedRecords`, `complete`, and inspected totals expose truncation. `omittedRecords` counts omitted records belonging to inspected documents; it does not claim to count the uninspected documents' history. More than 1,000 descendant collections fails explicitly. Archived documents are excluded; their historical assessments are not deleted.

`governance.assignments.apply` accepts the same inputs plus the returned `impactDigest`. It recomputes the preview, rejects incomplete evidence or changed impact, and invokes the ordinary assignment service. The digest covers working identities, effective policy differences, historical eligibility, and omission counts. Assignment and target-policy concurrency identities prevent stale writes. This provides a reviewable migration path; direct assignment remains available for managers who intentionally use the simpler existing operation.

Checklist progress includes append-only history for each stage item, ordered by its evidence ordinal. History supports audit display; the latest entry and current assignee still determine completion.
