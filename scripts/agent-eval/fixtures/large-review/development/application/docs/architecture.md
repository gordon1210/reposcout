# Architecture and ownership

`Application.request(method, path, credential, body)` is the public application boundary.
`api/routes.py` binds method/path patterns to actual handlers and required roles. Dispatch
resolves trusted context, checks role membership, validates the body, and runs a rollback-capable
in-memory transaction. It copies response data so callers cannot mutate stored state by editing
a previous response. Handlers return JSON-compatible values. Expected domain errors have an
HTTP-like status and a stable error code; the application opens no sockets.

The package areas have distinct responsibilities:

| Area | Owned behavior |
| --- | --- |
| `catalog`, `pricing`, `customers` | Visibility, search, component inventory, quote arithmetic, account state |
| `baskets`, `orders` | Versioned basket edits, checkout, frozen line values, ownership, order notes |
| `inventory` | Warehouse stock, reservation allocation, transfers, counts and movement ledger |
| `fulfillment` | Shipment, cancellation, return plans, pick lists and tracking |
| `billing` | Invoices, idempotent captures, credits, statements and reconciliation |
| `events` | Provider envelope routing, inbox identities, handler registry, outbox subscriptions and retries |
| `purchasing` | Supplier terms, receipts, invoice matching, replenishment and vendor credits |
| `recurring` | Consent snapshots, calendar cycles, checkout/retry and live subscription accounting |
| `logistics` | Reservation waves, cartons, courier contracts, manifests and claims |
| `reporting` | Projections from the same live order, stock, billing and event state |
| `api` | Authentication, route bindings, request adaptation and public projections |

The dataclass in `core/state.py` owns the stores. Services accept the shared state, trusted tenant
and necessary settings. Inventory and billing changes write their ledgers before a response is
returned. Domain services publish outbox events, while route-level authorization stays in the API.
The synthetic bootstrap creates products and customers, but creates no stock or order history.
Public tests construct their inventory and fulfillment histories through requests.

This is a synchronous reference implementation. Request rollback is implemented by copying the
small in-memory state; there is no concurrency or persistence guarantee across processes. Stable
business operations and their source dependencies, rather than the persistence adapter, are the
focus of the application. A review can legitimately establish a narrow change by following only
its active route, binding, mutation and response path plus pertinent checks.

[Connected operations](operations.md) describes how purchasing, recurring orders and warehouse
logistics reuse these same stock, order, invoice and shipment services.
