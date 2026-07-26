# Daemon and web dashboard

← [Documentation index](README.md)

The daemon watches one target, keeps the latest successful `ScanReport`, and serves it to the local
React dashboard. Source remains local; no hosted service is required.

## Start the daemon

```sh
reposcout daemon .
```

The default address is `http://127.0.0.1:7331`.

> The service has no authentication. Keep the loopback default unless every client on the chosen
> network may read source-derived repository metrics.

## Start the dashboard

The frontend uses the root pnpm workspace:

```sh
pnpm install
pnpm dev:web
```

Run the daemon and frontend in separate terminals. Vite proxies `/api` to the daemon.

The independent public landing page runs with:

```sh
pnpm dev:landing
```

## Daemon options

| Flag | Description | Default |
|---|---|---|
| `--host <ADDRESS>` | HTTP bind address | `127.0.0.1` |
| `--port <PORT>` | HTTP port | `7331` |
| `--debounce-ms <MS>` | Filesystem-event coalescing delay | `300` |
| `--profile <full\|lite>` | Analyzer set | `full` |

The `lite` profile omits whole-corpus duplication and Git churn. The report's
`analysis_profile` records those unavailable metrics so the dashboard can label them accurately.

## HTTP API

| Endpoint | Purpose |
|---|---|
| `GET /api/health` | Process health and version |
| `GET /api/snapshot` | Daemon state plus the latest successful report |
| `GET /api/graph?revision=N` | Build or reuse graph data for one completed revision |
| `GET /api/events` | SSE scan lifecycle events |
| `POST /api/rescan` | Queue a manual scan; repeated requests are coalesced |

SSE events are `scan_started`, `scan_completed`, and `scan_failed`.

## Runtime behavior

Scans are single-flight and execute away from the asynchronous HTTP runtime. Filesystem bursts
collapse into one pending refresh, and clients continue reading the last successful report while a
new scan runs.

Incremental caches remove repeated per-file work and reuse immutable Git commit events. A cold
whole-corpus duplication or large Git-history pass may still take time; the daemon reports state
and start time instead of imposing an arbitrary timeout.

Graph analysis is separately single-flight. Ordinary watched scans do not pay graph extraction
cost. Opening the Graph view requests one revision-keyed graph build, which is then reused.

## Dashboard views

The dashboard provides bookmarkable routes for:

- `/overview`
- `/risk`
- `/complexity`
- `/duplication`
- `/files`
- `/findings`
- `/graph`

Non-graph views use searchable, sortable, paginated data grids over the complete report.

## Graph explorer

The graph explorer converts mixed-language topology into a bounded navigation surface:

- architecture scopes derive from repository directories and package manifests;
- redundant single-child scope chains collapse;
- double-click enters a scope or file neighborhood;
- one click selects without changing browser history;
- breadcrumbs, Back, Forward, and refresh restore route state;
- search can focus any scanned file; and
- no view renders more than 100 nodes, including group containers.

Graph routes continue below `/graph`:

```text
/graph/scope/<repository-path>
/graph/file/<repository-path>
```

Non-default presentation, direction, and depth are stored in canonical query parameters.

### Relationships and layout

Explicit inheritance, implementation, trait, and embedding edges remain distinct from imports.
Files with strong resolved type reach open a semantic neighborhood with the dominant type centered
and relationship groups separated. Other files open the normal dependency/dependent neighborhood.

Focused cross-directory views create path-based parent groups labelled by module, package,
namespace path, or mixed-language scope. The details panel exposes only measured facts: language,
metrics, risk, churn, findings, symbols, callable complexity, test presence, resolver provenance,
and concrete connections.

Ambiguous names and unresolved imports remain diagnostics rather than speculative edges.

## Production builds

```sh
pnpm build:web
pnpm test:web
pnpm build:landing
```

Build outputs:

- dashboard: `apps/web/dist`
- landing page: `apps/landing/dist`
