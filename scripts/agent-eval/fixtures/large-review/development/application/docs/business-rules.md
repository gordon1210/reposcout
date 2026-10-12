# Business rules

## Tenants and identities

The authenticated credential selects tenant and actor. Body fields cannot select another tenant
or customer. Customer order and basket queries enforce ownership inside that tenant. Warehouse,
provider and administrator operations require their respective roles. Product codes, order IDs
and provider IDs may be reused by different tenants.

## Pricing and checkout

All money is integer EUR cents. TEA is tax exempt; MUG has domestic tax at 19 percent, rounded
half up per quote line after discount. Shipping is 499 cents domestically or 1299 internationally;
domestic goods subtotals of at least 10000 cents receive ordinary shipping free. Express adds
700 cents and cannot be combined with pickup. Pickup is free. A placed order preserves its
quoted prices if the catalog is later repriced.

The storefront WELCOME promotion is an exact, case-sensitive code, requires at least 2000 cents
of goods and applies only before the customer's first placed order; it deducts 500 cents.
TRADE10 is reserved for wholesale customers spending at least 10000 cents. Administrative
promotion preview is a separately authorized estimate. Preview may accept relaxed draft labels
and case variants, has no storefront minimum and cannot create or modify a customer charge.

Catalog bundles describe component availability. Customers order their components explicitly;
bundle SKUs never hold physical stock. A basket edit needs the current version. Successful
checkout can be replayed and returns its original order without reserving or charging twice.

## Stock and fulfillment

Warehouse priority is configured per tenant. An ordinary order must reserve every unit or the
request rolls back. A backorder-enabled order may reserve available stock, leave the rest
unallocated, and explicitly request allocation after later receipts. On-hand includes reserved
stock. Available stock equals on-hand minus current reservations. Stock counts and transfers
cannot consume another order's reserved units.

Shipment consumes the requested reserved units and physical units together, records shipment
history and leaves other reservations intact. Tracking may advance or repeat, but cannot move
backwards from delivered to in-transit.

Cancellation applies only to currently reserved, unshipped units. An accepted quantity must
increase the order's cancelled count by that quantity, release exactly that many reservations,
and increase free stock by the same quantity. Ordered and shipped counts do not change. Earlier
cancellations and shipments are historical facts and must not be released again. This
conservation rule applies to every warehouse layout and to orders partially fulfilled or
previously cancelled. The release ledger must record the same quantity as the inventory change.
Zero cancellation is a successful no-op. Negative, boolean or noninteger quantities are invalid;
a quantity above either remaining order units or current reservations is rejected atomically.

Returns are accepted through day 30 inclusive and only up to the unreturned quantity of the
selected shipment. Returned units do not erase shipment history. Unwanted and wrong-item returns
reenter saleable inventory; damaged returns do not. Returns and cancellations credit their
original goods value. Shipping is not automatically credited. Allocation of rounded credits uses
the cumulative cancelled or returned value so repeated operations conserve the original amount.

## Accounting and delivery

An invoice records the original total. Captures cannot exceed its open balance. A repeated
payment reference with the same order and amount is idempotent; conflicting reuse is rejected.
Credits reduce the amount due and expose any refundable paid balance. Events and audit entries
participate in the same request transaction as domain state. Failed requests cannot retain a
partial reservation, invoice, event receipt or audit entry.

[Provider event identity and retry behavior](events.md) and the
[fulfillment status decision order](fulfillment-states.md) are part of this contract.
