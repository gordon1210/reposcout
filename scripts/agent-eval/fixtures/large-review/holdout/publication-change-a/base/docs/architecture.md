# Application architecture

`app.mjs` exposes `createApplication`. `src/api/application.mjs` validates the dispatch envelope,
looks up an action in `src/api/routes.mjs`, executes a transaction, and returns detached data.
The route table is the application composition root; there is no automatic directory discovery
or dependency injection service. The same table registers authoring, publication, and administration
actions. A public share read is the one route that authenticates using its share token rather
than a workspace actor.

`storage/state` defines the in-memory tables, `storage/lookup` checks cross-entity references, and
`storage/transaction` restores a snapshot if an operation throws. Tables intentionally use plain
records so tests can inspect outcomes without a database. Identity allocation and audit sequence
numbers are deterministic. The event sequence describes application operations, not wall-clock time.

`permissions` resolves the account and nearest collection membership. `collections` maintains the
hierarchy and refuses cycles. `documents` owns stable identities and immutable revision chains;
read queries and history summaries are separate from mutation functions. The diff helper compares
two explicitly selected revisions and returns the smallest differing middle after common prefix
and suffix removal. It is a user-facing excerpt, not a general-purpose edit script.

`workflow` owns review state and approvals. `releases` validates approved entries and stores the
publication catalog. `content` normalizes and resolves requested document content, alongside
document links and heading outlines. `exports` creates manifests, queues jobs, materializes the
selected content, and renders one of three formats. A single rendering contract serves durable
exports and explicitly labelled working previews. Search indexing and share delivery reuse
publication selections, so they cannot independently decide which revision a release means.

`search` stores per-release entries and applies visibility before ranking. It has separate
tokenization, scoring, and request filtering. `preview` intentionally reads working content.
`comments` anchors feedback to a specific revision; `labels` tracks mutable organizational labels.
`subscriptions` records per-reader watches and builds event digests. Publication-only watches
follow release publication events; normal watches also see document edits and discussion.

`templates` and `importing` create or revise documents through the same document services.
`retention` computes an explicit deletion preview from protected references and recent revision
counts. `audit` filters the event journal and can render JSON or CSV. `admin` exposes user controls,
workspace totals, and a narrow referential-integrity report. None of these features runs a timer.

The project deliberately has no persistence adapter, HTML rendering, external identity provider,
or scheduler. Tests target one-shot domain behavior through dispatch; the synthetic implementation
does not claim production deployment or complete standards-compliant Markdown parsing.

The structure subsystem provides bounded source parsing, section edit planning, three-way merging, and provenance-aware discussion migration. Governance wraps ordinary review and publish routes when a collection policy applies, retaining source-pinned assessments, multi-stage review plans and append-only checklist evidence. Exchange reuses document and collection services for reference-aware batch import and receipt-based synchronization; published and working archives have distinct source contracts. These integrations are registered explicitly in the central route table.
