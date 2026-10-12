# Action reference

All actions receive an input object. The actor argument defaults to `admin` for local examples;
real checks pass an explicit actor whenever role behavior matters. Returned records use stable
IDs such as `document_0001`; callers should carry the returned IDs, not depend on a particular
counter. Every `id` below names the relevant resource, while `documentId`, `collectionId`, and
`releaseId` make relationships explicit.

| Action | Required input | Optional input or behavior |
|---|---|---|
| collections.create | name | parentId, public, description |
| collections.move | id | parentId; omitted means top-level |
| collections.list | none | parentId filter; includes breadcrumbs and descendant counts |
| members.assign | collectionId, actorId, role | nearest membership determines access |
| members.remove | collectionId, actorId | removes only the explicit membership |
| documents.create | collectionId, title, body | language, tags |
| documents.revise | id | title, body, language, tags, expectedRevisionId |
| documents.move | id, collectionId | authorization required in source and target |
| documents.archive | id | archived=false restores |
| documents.owner | id, ownerId | requires collection management |
| documents.get | id | revisionId, includeArchived |
| documents.list | none | collectionId, ownerId, includeArchived, limit, after |
| documents.history | id | returns revision summaries, newest first |
| documents.diff | id, before, after | both revisions must belong to id |
| documents.links | documentId | revisionId; unavailable targets do not disclose titles |
| documents.outline | documentId | revisionId; ignores fenced-code headings |
| reviews.open | documentId, assigneeId | revisionId, note |
| reviews.assign | id, assigneeId | open reviews only |
| reviews.decide | id, decision | approve or reject; optional note |
| reviews.get | id | includes unresolved comment count |
| reviews.queue | none | assigneeId, status, limit, after |
| releases.publish | name, entries | each entry supplies approvalId and optional documentId |
| releases.withdraw | id | reason |
| releases.get | id | includes exact approved entry identities |
| releases.list | none | includeWithdrawn, limit, after |
| exports.create | releaseId, requestKey | format=json, text, or csv |
| exports.cancel | id | pending jobs only |
| exports.run | id | creates one artifact or returns prior completion |
| exports.get | id | returns job and artifact, if complete |
| exports.list | none | status, limit, after; actor-owned jobs only |
| search.rebuild | releaseId | replaces entries for that release |
| search.query | query | releaseId, language, tag, limit |
| search.save | name, query | language, tag |
| search.saved | id | executes the owner's stored query |
| search.delete | id | owner only |
| preview.document | documentId | current working revision; includeHistory adds prior revision summaries |
| preview.bundle | documentIds | name, format; never persists an artifact |
| shares.create | releaseId | maxViews, default 100 |
| shares.read | token | no workspace actor required |
| shares.revoke | id | creator or administrator |
| comments.add | documentId, body | revisionId, reviewId, line |
| comments.resolve | id | resolved=false reopens |
| comments.list | documentId | revisionId, includeResolved |
| labels.set | documentId, labels | replacement; normalized and unique |
| labels.find | label | visible, nonarchived documents |
| labels.counts | none | visible, nonarchived document counts per label |
| watches.add | documentId | mode=all or publication |
| watches.remove | documentId | only the caller's watch |
| watches.list | none | caller's watch state |
| digest.preview | none | does not advance watch cursors |
| digest.deliver | none | persists a delivery and advances caller's cursors |
| audit.list | none | afterSequence, limit, actorId, target, action |
| audit.export | none | audit filters and format=json or csv |
| retention.set | collectionId, keepRecent | positive recent revision count |
| retention.preview | collectionId | describes eligible unreferenced revisions |
| retention.apply | collectionId | deletes only preview-eligible revisions |
| templates.create | collectionId, name, titlePattern, bodyPattern | required parameter names |
| templates.instantiate | id, values | collectionId, tags |
| templates.list | collectionId | template metadata, not full body |
| import.preview | collectionId, source | parses without creating a document |
| import.apply | collectionId, source | documentId, expectedRevisionId for revisions |
| users.create | id, displayName | role |
| users.active | id, active | administrator safeguards apply |
| users.list | none | includeInactive |
| workspace.statistics | none | counts only visible collection content |
| workspace.integrity | none | administrator-only reference check |

List cursors are exact last-seen IDs from an unchanged result set; absent cursors are errors rather
than silently restarting from the beginning. Limits are bounded and positive. Search uses its own
relevance order and returns a total plus a bounded list instead of a cursor.
