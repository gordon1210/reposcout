# Business contracts

## Documents and revisions

A document is a stable identity located in one collection. A revision is an immutable title,
body, language, and tag set belonging to exactly one document. Editing creates another revision
and advances the document's current pointer. It never rewrites the old body. Revision identifiers
are workspace-unique. An optional expected revision provides optimistic concurrency: a stale
request fails without creating a revision or audit entry. Moving or relabelling a document does
not create a content revision. Revision tags travel with content; workspace labels describe the
document independently and are not exported as revision tags.

## Review and publication

A review refers to one revision selected when the review is opened. Authors cannot approve their
own revision. Only the assigned reviewer can decide, and unresolved review comments prevent
approval. Later editing does not silently retarget an open review. An approval records that exact
document/revision relationship. Publishers select approvals to create a release containing at
most one entry for each document. Release entries do not advance when a document is edited or
when a later release is created.

Every external representation of a release must use the revisions selected by that release.
This applies to JSON/text/CSV exports, public share reads, and the release's search index. The
title, body, language, tags, checksum, and revision identifier must describe the same selected
content. Export queueing and export execution may happen between different document edits;
the queued release identity does not change. A later release may legitimately select a newer
approval while an earlier release continues to describe its own entries.

Working-copy preview, ordinary document reads without an explicit revision, link targets without
a revision suffix, and notification digest titles intentionally use the current working revision.
They can show unapproved content to an authorized workspace reader. Their behavior is distinct
from a release representation. A preview is marked as such and does not create a published
artifact, release, or approval.

## State transitions and side effects

Mutation handlers execute atomically. A domain validation failure must leave document state,
identity counters, and audit history as they were before the request. Responses and inspection
snapshots are detached from live storage. Domain errors have stable codes; incidental exception
messages are not an API contract.

An export request key is scoped to its actor. Replaying the same request returns the original job;
reusing a key for another release or format fails. Re-running a complete job returns the existing
artifact. A pending job may be cancelled; a complete or cancelled job cannot start again.
Withdrawing a release disables new exports and pending jobs, revokes its shares, and removes its
search entries. Existing artifact records remain available to the job owner for audit.

## Access and preservation

Private collection access follows the nearest explicit membership in the ancestor chain. An
explicit child membership replaces inherited membership. Public collections allow reads but not
arbitrary edits. Independent release sharing grants only that release's content, not general
workspace access. Share view limits and revocation apply before any content is delivered.

Retention may remove unreferenced old revisions beyond the configured recent count. Current
revisions, approvals, reviews, release entries, and comment anchors keep their source revisions.
Archiving a document hides it from ordinary listings and editing but does not erase history.
An open review must be closed before the document is archived.

## Governed review evidence

Configured checklist item identifiers are local to their review stage. Two stages may use the same item identifier for different verification duties. Each stage requires its own recorded completion by its current assigned reviewer; a matching item identifier or actor in another stage does not transfer that evidence. A stage may allow the same reviewer as an earlier stage unless distinctReviewer is explicitly required. Historical attestations stay readable, while decision and publication gates use the evidence for the current stage and policy source.
