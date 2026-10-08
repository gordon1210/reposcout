# Publication interchange

Interchange is an explicit batch operation over the normal collection, document,
revision, authorization, and release services. Import creates drafts or appends
revisions. It never imports approvals, releases, owners, memberships, or public
visibility. Imported documents follow the ordinary review and publication workflow.
The existing single-document import and export actions retain their contracts.

## Manifest version 1

A manifest is an object, or a JSON string containing that object:

```json
{
  "format": "publication-exchange",
  "version": 1,
  "origin": "authoring:operations",
  "collections": [{ "key": "manual", "name": "Operations" }],
  "documents": [
    { "key": "intro", "collectionKey": "manual", "title": "Introduction",
      "body": "See [procedure](exchange:procedure).", "language": "en", "tags": [] },
    { "key": "procedure", "collectionKey": "manual", "title": "Procedure",
      "body": "Follow the steps." }
  ]
}
```

Collection keys and document keys are unique within their respective kinds. Keys
contain one to eighty ASCII letters, digits, periods, underscores, or hyphens and
start with a letter or digit. Collection `parentKey` defaults to null; ancestry
must be acyclic and every parent must be included. Document collections must be
present. Collection `description` defaults to empty. Language defaults to `en`
and must be a language tag. Tags are bounded, deduplicated, and sorted by the
normal document rules. Unknown fields and unsupported versions are rejected,
not silently discarded or guessed. Version 1 is the only supported version;
there is no implicit legacy migration.

Documents may carry `source` containing original `documentId`, `revisionId`, and
SHA-256 `checksum`. These identify the source revision before portable references
are rewritten. They are provenance claims, not authorization or cryptographic
signatures. Manifest identity covers normalized portable content and provenance.
Collections are sorted with parents first and documents by key, making equivalent
input ordering produce the same manifest identity.

Limits are 200 documents, 100 collections, 200,000 characters per body, and
4,000,000 characters for the normalized manifest. Empty bodies are valid. Unused
collection declarations are validated but are not created by an import.

## Preview and apply

`exchange.import.preview` accepts exactly one `manifest` or `archive`, optional
`collectionBindings`, `parentCollectionId`, `targets`, and `policy`.

`collectionBindings` maps a portable collection key to an existing collection ID.
An explicit binding is authoritative: the source name and ancestry do not rename
or move that destination. Unbound collections are created privately under their
resolved source parent, or under `parentCollectionId` for source roots. Creating
collections requires the existing manage permission; importing into a bound
collection requires edit permission. No implicit name matching occurs.

`targets` maps document keys to `{ documentId, expectedRevisionId }`. Both fields
are required for updates. A target must still have that current revision and
must belong to the resolved collection. Duplicate destination documents are
rejected. Missing targets mean create. Updates append normal immutable revisions
and preserve the previous revision and all existing review state.

Preview executes the same import pipeline against a cloned state. It returns
`planToken`, `manifestIdentity`, `sequence`, `applicable`, `imported`, `rejected`,
`diagnostics`, and `collections`, without changing counters, events, or documents.
Predicted destination identities are explanatory until application commits.

`exchange.import.apply` repeats the preview input and adds `planToken` plus a
nonempty `requestKey`. The token binds normalized content, mappings, policy,
actor, and workspace sequence. Any intervening normal mutation requires another
preview, even an unrelated mutation. This conservative rule avoids applying a
plan whose permission, identity allocation, or target assumptions changed.

A successful request records one receipt and an `exchange.imported` audit event,
as well as the normal collection/document events. Repeating the same request key
for the same actor and normalized request returns the original receipt with
`replayed: true` and no mutations; current edit access is rechecked. Reusing that
key with different input fails. The first request returns `replayed: false`.
`exchange.import.get` takes the receipt ID; its creator or an administrator may
read it, subject to current read access to every imported document.

## References and batch policy

Portable references use `[label](exchange:key)`. All referenced keys must be
included. On import, they become ordinary `[label](doc:document_0001)` links to
the destination IDs. Forward references, cycles, and self references work without
placeholder revisions. Source `doc:` references are retained only after normal
read authorization and revision ownership checks succeed. The syntax is the
same lightweight inline-link model used by the application's link checker; this
is not a full Markdown AST or a generic URI rewriter.

`policy: "atomic"` is the default. Any invalid reference, permission failure,
collection conflict, or target revision conflict prevents the entire batch.
Failed application restores all counters, collections, revisions, and events.

`policy: "partial"` partitions documents into connected components of portable
references, treating edges as undirected for commit grouping. Each component is
transactional. If A links to B and B fails, both are rejected; unrelated C can
commit. Collections required by a successful component are shared with later
components. A failed component rolls back its newly created collections and
normal service events. A batch with no successful component fails without a
receipt. Structural manifest errors, including duplicate keys and collection
cycles, always reject the complete request before component processing.

Diagnostics distinguish malformed references, missing portable targets,
collection mismatches, stale revisions, and normal service permission errors.
Component errors include affected keys. Preview and apply use identical
component ordering and decision logic.

## Release and working-copy archives

`exchange.export.release` requires `releaseId`. It resolves the release's retained
entries through the release catalog, checks that the release is not withdrawn,
and reads each exact `revisionId`. The current working revision never substitutes
for a release revision. Retained body checksums must match the release entries.
Read access to each document and included collection ancestor is required.

`exchange.export.working` requires an explicit nonempty `documentIds` list. It
captures each selected document's current revision and labels the result
`mode: "working"`. There is no automatic release/working fallback. Both routes
return `manifest`, `archive`, `identity`, `sources`, and reference `diagnostics`.
Repeated calls with unchanged source facts return identical output.

References to included documents become portable links if an explicit referenced
revision matches the selected revision. A reference outside the selected set, or
to a different selected document revision, fails by default. Explicit
`externalPolicy: "preserve"` retains it as a `doc:` link and emits a diagnostic.
A later import must still authorize and resolve that external destination; IDs
from another workspace are not silently rebound.

Archives are deterministic JSON envelopes, not ZIP files or filesystem writes.
Version 1 contains a normalized `manifest.json`, a Markdown file per document,
and a catalog recording path, media type, character count, UTF-8 byte count, and
SHA-256 checksum. The archive identity binds the catalog; the manifest identity
binds normalized domain content. Rendered Markdown has JSON-encoded metadata
values in its header and the portable document body.

`exchange.archive.inspect` validates the supported envelope, bounded safe paths,
unique files, exact catalog, both identities, and every file against the normalized
manifest. It returns a catalog summary. Import accepts the same archive envelope
and performs these checks before planning. Tampered content, missing files,
unlisted files, traversal paths, and altered catalogs are rejected. Integrity
checks detect inconsistent transport; they do not authenticate an untrusted
sender who can replace both content and hashes.

## Incremental synchronization

Synchronization compares a new manifest or archive to explicitly supplied
baseline anchors. It does not search by title or infer that an equal body belongs
to an existing document. `exchange.import.anchors` accepts an import receipt ID
and returns its portable-key `anchors` and `collectionBindings`, suitable for the
next preview. Each anchor contains destination `documentId`, destination
`revisionId`, normalized portable `sourceIdentity`, and `referenceIdentity`.
Source identity includes content, metadata, key, and declared provenance. Reference
identity covers the ordered portable links and their destination document IDs.

`exchange.sync.preview` accepts the ordinary manifest/archive, collection mapping,
parent destination, and atomic/partial policy inputs, plus `anchors` and optional
`createMissing: true`. Ordinary import `targets` are rejected here to avoid two
competing destination policies. Anchors must name distinct documents and carry
complete SHA-256 identities. The preview produces one decision per incoming key:

- `unchanged`: incoming source and reference identities match the anchor, and the
  destination still has the anchored current revision.
- `create`: the source has no anchor and creation was explicitly enabled.
- `revise`: the destination still has its anchored revision, but incoming content,
  provenance, or reference bindings changed.
- `conflicting`: the destination changed locally, is archived/unavailable, lacks
  edit permission, disagrees with an explicit collection mapping, or has an
  invalid portable reference.
- `unmapped`: there is no anchor and creation was not enabled.

Local edits count as conflicts even if the incoming source itself is unchanged.
There is no overwrite or automatic merge option. Resolve the local edit outside
synchronization and supply a deliberately accepted new anchor if appropriate.
Changing a collection binding cannot silently move an existing document.

Reference-connected components remain the unit of eligibility. A conflict or
unmapped key blocks its whole component; atomic policy blocks every component if
any component is blocked. Preview includes `components`, `counts`, `decisions`,
`blocked`, `diagnostics`, predicted writes, rejected writes, the complete incoming
`sourceIdentities`, and a `planToken`. It simulates normal service operations as
well, so collection name conflicts or normal validation failures appear before
application. Unchanged documents receive no new revisions. References from a
changed document to an unchanged destination bind directly to that destination;
changed components still apply together, including when unchanged nodes connect
their references.

`exchange.sync.apply` repeats the request with `planToken`, `requestKey`, and
`expectedSourceIdentities` copied exactly from preview. The complete incoming
identity map must match; missing keys, extra keys, and altered identities reject
the operation. Plan tokens bind the actor, workspace sequence, normalized
manifest, anchors, collection mappings, creation option, and batch policy.
Application uses the existing import transaction engine for each reference
component, wrapped by one outer transaction for atomic rollback.

The receipt contains new anchors for successful writes, retained anchors for
unchanged/failed/retired sources, and the decisions and diagnostics. Failed partial
components keep their previous anchors for a later retry. A completely unchanged
eligible synchronization records its receipt and audit event but creates no new
document revisions. An entirely ineligible request fails without a receipt.
Request-key replay returns the same receipt without writes after access checks.
`exchange.sync.get` retrieves a receipt for its creator or an administrator,
subject to current document read permissions.

Anchor keys absent from the new source appear in `retired`. This is advisory:
synchronization never deletes, archives, moves, or withdraws a destination merely
because its source disappeared. No old revision, approval, or release is changed.
