/**
 * Optional browser-side daemon auth helpers.
 *
 * Prefer the Vite dev-server proxy, which injects the token from the owner-only
 * daemon token file and never needs a browser-visible secret. These helpers
 * remain for direct (non-proxied) clients.
 *
 * Token sources (in order):
 * 1. URL fragment `#token=...` (not sent to servers)
 * 2. One-time `?token=` query, immediately stripped via `history.replaceState`
 *    and stored in sessionStorage for the tab
 * 3. sessionStorage from a previous migration in this tab
 */

const SESSION_KEY = "reposcout_daemon_token"
const DAEMON_TOKEN_PATTERN = /^[0-9a-f]{64}$/

function readFragmentToken(): string | null {
  if (typeof window === "undefined") return null
  const raw = window.location.hash.replace(/^#/, "")
  if (!raw.includes("=")) return null
  const token = new URLSearchParams(raw).get("token")
  return token && DAEMON_TOKEN_PATTERN.test(token) ? token : null
}

function migrateQueryToken(): string | null {
  if (typeof window === "undefined") return null
  const url = new URL(window.location.href)
  const fromQuery = url.searchParams.get("token")
  if (!fromQuery || !DAEMON_TOKEN_PATTERN.test(fromQuery)) return null

  try {
    sessionStorage.setItem(SESSION_KEY, fromQuery)
  } catch {
    // sessionStorage may be unavailable; still strip the query.
  }

  url.searchParams.delete("token")
  // Prefer a fragment so reloads keep the token without server-visible query logs.
  url.hash = `token=${encodeURIComponent(fromQuery)}`
  window.history.replaceState(
    null,
    "",
    `${url.pathname}${url.search}${url.hash}`
  )
  return fromQuery
}

function readSessionToken(): string | null {
  if (typeof window === "undefined") return null
  try {
    return sessionStorage.getItem(SESSION_KEY)
  } catch {
    return null
  }
}

export function daemonAuthToken(): string | null {
  const fromFragment = readFragmentToken()
  if (fromFragment) {
    try {
      sessionStorage.setItem(SESSION_KEY, fromFragment)
    } catch {
      // ignore
    }
    return fromFragment
  }

  const migrated = migrateQueryToken()
  if (migrated) return migrated

  return readSessionToken()
}

export function daemonAuthHeaders(extra?: HeadersInit): Headers {
  const headers = new Headers(extra)
  const token = daemonAuthToken()
  if (token) {
    headers.set("Authorization", `Bearer ${token}`)
  }
  return headers
}

export function daemonEventsUrl(path = "/api/events"): string {
  const token = daemonAuthToken()
  if (!token) return path
  // SSE still needs a query token on the wire for loopback EventSource clients;
  // headers cannot be set on EventSource. Prefer the proxy path in normal dev.
  const separator = path.includes("?") ? "&" : "?"
  return `${path}${separator}token=${encodeURIComponent(token)}`
}
