# Connected operational workflows

The same tenant-scoped commerce state supports three adjacent operational areas. They are
explicit request workflows; planning and dispatch do not run autonomously.

## Purchasing and replenishment

An administrator registers and qualifies a supplier, records SKU terms, drafts a purchase order
and approves its versioned terms. Approved prices remain fixed. Warehouse receipts distinguish
accepted, quarantined and rejected quantities. Accepted units enter the ordinary inventory
ledger through `receive_stock`; an explicit allocation option can fill existing order
backorders through the same reservation service used at checkout. Inspection later moves
quarantined units into accepted stock or rejected stock, without double receipt.

Supplier invoice matching compares approved terms, accepted receipt quantities and claimed
invoice quantities. Matched invoices cannot reuse quantities already claimed by another invoice.
Corrections happen before matching. Vendor concessions for rejected units can reduce an unpaid
matched supplier invoice; they do not create customer refunds or physical stock. Reconciliation
checks purchasing receipts against the ordinary stock ledger.

Replenishment combines free stock, incoming approved quantities, warehouse targets and explicit
backorder rules. Pack sizes round proposals to purchasable quantities. A saved proposal records
the inventory/term basis it used and rejects conversion when that basis changes. Routes are
under `/purchasing`; the public purchasing checks demonstrate the full request sequence.

## Recurring orders

Recurring plans describe items, calendar cadence and retry policy. Enrollment snapshots customer
consent; later plan edits do not silently rewrite it. A customer can pause, skip, resume, stop
or explicitly schedule a compatible plan change. Calendar schedules preserve their anchor,
including month-end and leap-year dates.

An administrator requests a bounded due-cycle plan, then explicitly executes a cycle. Execution
uses the enrolled customer's trusted identity and the existing order-creation service, so stock,
current prices, tax, invoices, audit and outbox effects follow ordinary checkout. No automatic
payment or prepaid entitlement is invented. A replay returns the existing order. Failed checkout
restores commerce state before recording retry history; stock shortages can retry, while other
failures require explicit intervention. Customer statements project actual orders and invoice
balances, including later fulfillment changes. Customer routes use `/subscriptions`; operations
use `/admin/recurring`.

## Warehouse logistics

A wave claims work from existing reservations in one warehouse. Picking validates the planned
items; cartons record measured weight, dimensions and contents. Packing cannot create stock or
bypass reservations. Courier comparisons apply service constraints, dimensional weight, price
bands, fuel and insurance. Manifest creation snapshots the selected carrier contract so later
repricing cannot alter an accepted dispatch estimate.

Closing and dispatching a manifest validates all cartons. Dispatch calls the ordinary shipment
service, so failures roll back earlier cartons' stock, shipment, ledger and audit changes in the
same request. Customer shipment projections enforce ordinary order ownership. Delivery exceptions
and courier claims have separate operational lifecycles; a courier disposition does not itself
perform a customer return or refund. Claims distinguish approved recovery from money actually
recovered. Operational routes use `/logistics`; customers read `/orders/{id}/shipments`.
