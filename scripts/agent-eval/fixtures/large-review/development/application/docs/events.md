# Provider inbox and local outbox

A provider submits a named event to `POST /events/{provider}` with an event ID, a registered
`type`, and an object `data`. The route authenticates a provider credential and checks that it
may submit for that provider. The authenticated context supplies the tenant; payload tenant
fields are not authoritative.

Event identity is the tuple **(tenant, provider, event_id)**. An identity applies at most once.
Different tenants may legitimately reuse the same provider event IDs, and different providers
may legitimately reuse an event ID within a tenant. A duplicate response reports `applied:
false` and `duplicate: true`; it performs no domain mutation. A successful first application
reports `applied: true`, `duplicate: false`, and the handler's public outcome.

Registered inbound types are `stock.received`, `payment.captured`, `credit.requested`, and
`shipment.tracked`. The inventory, invoice or tracking endpoints expose the resulting domain
state; a receipt counter alone is not the business effect. A malformed or rejected event does
not reserve its identity. A corrected request can reuse that identity and apply successfully.
The handler completes before the receipt is committed, within the request transaction.

Outbound domain messages use configured local subscriptions. Outbox draining is an explicit
one-shot administrative operation, with a caller-provided nonnegative tick. The demonstration
driver either acknowledges delivery or simulates rejection; it never contacts a network.
Rejected deliveries use exponential delays, capped at 64 ticks, and at most five attempts.
Delivered destinations are not delivered again. Analytics payloads omit payment references.


## Delivery-manifest reconciliation

Administrators can submit `events: [{event_id, type}, ...]` to
`POST /admin/events/{provider}/reconcile`. The read-only report compares at most 100 declared
events with receipts already applied for that authenticated tenant and provider. It returns
matched handler outcomes, missing IDs, type conflicts and previously applied but unlisted IDs.
Duplicate manifest IDs are invalid. Reconciliation never applies the events again or changes
stock, invoices or receipt history.

The manifest index is built after filtering receipts to one tenant. Provider/event-ID receipt
keys are sufficient within that index. Live ingestion retains receipts in the application's
shared inbox and must still preserve the full event-identity contract above. The same provider
normalization can be useful in both workflows only when each consumer preserves its own scope.
