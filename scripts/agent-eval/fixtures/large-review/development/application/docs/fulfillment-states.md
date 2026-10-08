# Fulfillment state projection

Orders are nonempty. Their public status is derived from line counters and current reservations;
it is not stored independently. Sum each counter over all lines, then use this priority:

| Condition | Status |
| --- | --- |
| Cancelled equals ordered | `cancelled` |
| Shipped plus cancelled equals ordered; all shipped units returned | `returned` |
| Shipped plus cancelled equals ordered; some shipped units returned | `partially_returned` |
| Shipped plus cancelled equals ordered; none returned | `fulfilled` |
| Some units shipped and some still outstanding | `partially_shipped` |
| No units shipped, with current reservations | `reserved` |
| No units shipped and no current reservations | `awaiting_stock` |

A return does not reduce the shipped counter. A partly shipped order remains partly shipped
while other ordered units remain outstanding, even if some earlier shipments have been returned.
Mixed shipment and cancellation can finish an order. The returned state requires at least one
shipped unit; an entirely cancelled order stays cancelled.

Customer order detail, cancellation and return responses, and administrative fulfillment
reports use the same status projection. Extracting the projection into a helper must preserve
these observable values and all unrelated response fields. Staff promotion-preview estimates
are an independent route and have no role in choosing customer fulfillment status or price.
