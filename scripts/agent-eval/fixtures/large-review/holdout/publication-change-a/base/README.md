# Folio publication workspace

Folio is an in-memory document collaboration application. Editors maintain working documents in
collections, independent reviewers approve individual revisions, and publishers group approved
revisions into releases. Readers can export a release, search published content, or use a limited
share. Workspace previews, comments, labels, templates, and imports support authoring work.

The package uses native JavaScript ES modules and Node's standard library. There is no HTTP
server, external database, package installation, network request, system clock, or background
worker. `createApplication()` from `app.mjs` returns a fresh isolated workspace. Call
`dispatch(action, input, actorId)` to exercise the same registered handlers used by the checks.
Commands return `{ ok: true, data }` or `{ ok: false, error: { code, message, details } }`.
Unexpected programming errors remain exceptions. `inspect()` returns a detached snapshot for
diagnostics; mutations to returned data do not alter stored application state.

Run all checked-in application checks in one process:

```sh
node tests/run.mjs
```

The runner imports checks serially. Each check creates its own workspace and needs no services.
Individual check modules export a default function, so a focused check can also be imported and
called directly with Node. `tests/support.mjs` contains ordinary public setup helpers and assertions.

Start with [business contracts](docs/business-contracts.md) for behavior and
[architecture](docs/architecture.md) for module responsibilities. The [action reference](docs/actions.md)
describes public request shapes. Focused guides explain [publication](docs/publication.md),
[reviews](docs/reviews.md), [exports](docs/exports.md), [access](docs/access.md),
[search](docs/search.md), [retention](docs/retention.md), and
[imports and templates](docs/imports-and-templates.md).

All users, documents, content, and history in this repository are synthetic. The initialized
workspace contains four active accounts: `admin`, `editor`, `reviewer`, and `reader`. Collections
must explicitly grant access; global editor/reviewer roles do not grant access to every private
collection. Tests assign the required memberships through the public API.

The collaboration workspace also supports [structured edits and discussion migration](docs/structure.md), [governed review and policy evolution](docs/governance.md), and [document interchange and synchronization](docs/exchange.md). These routes use the same transactional document, review, and publication services.
