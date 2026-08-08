# Daemon and web dashboard

← [Documentation index](README.md)

The daemon watches one target, keeps the latest successful `ScanReport`, and serves it to the local
React dashboard. Source remains local; no hosted service is required.

## Start the daemon

```sh
reposcout daemon .
```

The default address is `http://127.0.0.1:7331`.

## Authentication and network boundary

The daemon requires a fresh bearer token by default, including on loopback. After successfully
binding its port, it writes the token to the platform runtime directory or RepoScout cache as
`daemon-<port>.token` and prints that path. The file is owner-only on Unix and is written with
symlink and regular-file checks; keep it secret because the API exposes repository-derived
information.

Requests may send `Authorization: Bearer <token>` or `X-RepoScout-Token: <token>`. Loopback SSE
clients may instead use `?token=` on `/api/events` because browser `EventSource` cannot set an
authorization header; query tokens are rejected for every other endpoint and for remote
listeners. Host and Origin validation remain an additional browser and DNS-rebinding boundary.

Keep the loopback default for ordinary use. `--unsafe-no-auth` deliberately disables the token but
is accepted only on loopback. A non-loopback listener requires `--allow-insecure-remote`, still
requires token authentication, and sends that token over plain HTTP; use this mode only behind a
TLS reverse proxy.

## Start the dashboard

The frontend uses the root pnpm workspace:

```sh
pnpm install
pnpm dev:web
```

Run the daemon and frontend in separate terminals. Vite proxies `/api` to the daemon.
The proxy reads the port-scoped token file with no-follow and size/type checks and injects the
bearer token server-side, so it never needs a `VITE_*` browser variable. For a non-default daemon
port, set `REPOSCOUT_DAEMON_PORT` for the Vite command; specialized setups may point
`REPOSCOUT_DAEMON_TOKEN_FILE` at the exact token file.

Direct, non-proxied browser clients can receive a valid token through a `#token=...` fragment. A
one-time `?token=...` value is immediately removed from the URL query and retained in the fragment
and tab-local `sessionStorage`; the proxy path above remains preferable for local development.

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
| `--profile <full\|lite\|safe>` | Analyzer and trust profile | `full` |
| `--no-project-config` | Ignore repository-owned configuration | off |
| `--unsafe-no-auth` | Disable bearer-token authentication on loopback | off |
| `--allow-insecure-remote` | Permit authenticated non-loopback binding over plain HTTP for use behind a TLS proxy | off |

The `lite` profile omits whole-corpus duplication and Git churn. The report's
`analysis_profile` records those unavailable metrics so the dashboard can label them accurately.
The `safe` profile also ignores project configuration and applies the same conservative
file/byte/time, worker, history, context, discovery, and health limits as a safe CLI scan.

## HTTP API

| Endpoint | Purpose |
|---|---|
| `GET /api/health` | Process health and version |
| `GET /api/snapshot` | Daemon state plus the latest successful report |
| `GET /api/graph?revision=N` | Build or reuse graph data for one completed revision |
| `GET /api/events` | SSE scan lifecycle events |
| `POST /api/rescan` | Queue a manual scan; requires `X-RepoScout-Request: rescan` |

SSE events are `scan_started`, `scan_completed`, and `scan_failed`.

Unless `--unsafe-no-auth` is active, every endpoint requires the daemon token. The custom rescan
header is required in addition to authentication and prevents a cross-origin browser page from
issuing a simple request. Rescans have a one-second cooldown in addition to single-flight
coalescing, and the daemon accepts at most 32 concurrent SSE clients.

## Runtime behavior

Scans are single-flight and execute away from the asynchronous HTTP runtime. Filesystem bursts
collapse into one pending refresh, and clients continue reading the last successful report while a
new scan runs.

Incremental caches remove repeated per-file work and reuse immutable Git commit events. A cold
whole-corpus duplication or large Git-history pass may still take time; daemon scans apply the
configured cooperative deadline and expose any truncation in the report diagnostics.

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
The risk view identifies the report's risk-algorithm version. The duplication view prefers the
compact production-source projection for reports carrying production evidence and falls back to
the legacy all-health-corpus projection only for older reports; raw groups remain part of the
daemon snapshot.

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
pnpm lint:frontend
pnpm build:web
pnpm test:web
pnpm build:landing
```

Build outputs:

- dashboard: `apps/web/dist`
- landing page: `apps/landing/dist`
