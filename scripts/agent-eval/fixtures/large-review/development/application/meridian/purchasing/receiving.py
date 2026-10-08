"""Physical receipt and inspection; quarantined units never enter sellable stock."""
from meridian.core.errors import require
from meridian.core.audit import record
from meridian.core.quantities import quantity
from meridian.inventory.adjustments import receive_stock
from meridian.inventory.reservations import reserve_order
from meridian.inventory.availability import reserved_for_order
from meridian.orders.queries import backorders
from meridian.orders.repository import get_order
from meridian.events.outbox import publish
from meridian.purchasing.orders import purchase_order, remaining, refresh_status
from meridian.purchasing.validation import unique_skus, text, integer, boolean, snapshot, lookup, version


def allocate_shortages(state, settings, tenant):
    """Use the existing shortage and reservation flows in deterministic order."""
    shortages = backorders(state, tenant, lambda oid, sku: reserved_for_order(state, tenant, oid, sku))
    allocated = []
    for shortage in shortages:
        order = get_order(state, tenant, shortage["order_id"])
        added = reserve_order(state, settings, order, allow_partial=True)
        if added:
            allocated.append({"order_id": order.order_id, "quantity": sum(row.reserved for row in added)})
    return allocated


def receive(state, settings, context, order_id, body):
    order = purchase_order(state, context.tenant, order_id)
    reference = text(body.get("delivery_reference"), "delivery_reference", 80)
    allocate = boolean(body.get("allocate_backorders", False), "allocate_backorders")
    raw_lines = unique_skus(body.get("lines"))
    normalized = {}
    for sku, raw in raw_lines.items():
        accepted = integer(raw.get("accepted", 0), "accepted", 0, 1_000_000)
        quarantined = integer(raw.get("quarantined", 0), "quarantined", 0, 1_000_000)
        rejected = integer(raw.get("rejected", 0), "rejected", 0, 1_000_000)
        quantity(accepted + quarantined + rejected)
        reason = text(raw.get("reason"), "reason") if quarantined or rejected else None
        normalized[sku] = {"sku": sku, "accepted": accepted, "quarantined": quarantined,
                           "rejected": rejected, "reason": reason}
    request = {"lines": normalized, "allocate_backorders": allocate}
    for (tenant, _), previous in state.purchase_receipts.items():
        if tenant == context.tenant and previous["purchase_order_id"] == order_id and previous["delivery_reference"] == reference:
            require(previous["request"] == request, "receipt_conflict", "Delivery reference was used with different content", 409)
            return receipt_view(previous)
    version(order, body.get("expected_version"))
    require(order["status"] in {"approved", "partially_received"}, "purchase_status", "Purchase is not open for receiving", 409)
    for sku, line in normalized.items():
        require(sku in order["lines"], "purchase_line_missing", "SKU was not ordered", 409)
        total = line["accepted"] + line["quarantined"] + line["rejected"]
        require(total <= remaining(order["lines"][sku]), "over_receipt", "Delivery exceeds the open purchase quantity", 409)
    receipt_id = state.next_id(context.tenant, "receipt")
    row = {"receipt_id": receipt_id, "tenant": context.tenant, "purchase_order_id": order_id,
           "supplier_id": order["supplier_id"], "warehouse": order["warehouse"],
           "delivery_reference": reference, "request": snapshot(request), "lines": normalized,
           "version": 1, "inspections": [], "allocations": []}
    for sku, line in normalized.items():
        purchased = order["lines"][sku]
        line["unit_cents"] = purchased["unit_cents"]
        line["received"] = line["accepted"] + line["quarantined"] + line["rejected"]
        line["credited_quantity"] = 0
        for field in ("accepted", "quarantined", "rejected", "received"):
            purchased[field] += line[field]
        if line["accepted"]:
            receive_stock(state, settings, context.tenant, order["warehouse"], sku, line["accepted"], receipt_id)
    state.purchase_receipts[(context.tenant, receipt_id)] = row
    order["version"] += 1
    refresh_status(order)
    if allocate:
        row["allocations"] = allocate_shortages(state, settings, context.tenant)
    publish(state, context.tenant, "purchase.received", {"receipt_id": receipt_id, "purchase_order_id": order_id})
    record(state, context, "purchase.received", order_id, {"receipt_id": receipt_id})
    return receipt_view(row)


def inspect(state, settings, context, receipt_id, body):
    row = lookup(state.purchase_receipts, context.tenant, receipt_id, "receipt")
    version(row, body.get("expected_version"))
    order = purchase_order(state, context.tenant, row["purchase_order_id"])
    allocate = boolean(body.get("allocate_backorders", False), "allocate_backorders")
    reason = text(body.get("reason"), "reason")
    dispositions = unique_skus(body.get("lines"))
    prepared = []
    for sku, raw in dispositions.items():
        require(sku in row["lines"], "receipt_line_missing", "SKU is not on this receipt", 409)
        accepted = integer(raw.get("accepted", 0), "accepted", 0, 1_000_000)
        rejected = integer(raw.get("rejected", 0), "rejected", 0, 1_000_000)
        quantity(accepted + rejected)
        require(accepted + rejected <= row["lines"][sku]["quarantined"],
                "inspection_exceeds_quarantine", "Disposition exceeds quarantine balance", 409)
        prepared.append((sku, accepted, rejected))
    for sku, accepted, rejected in prepared:
        for line in (row["lines"][sku], order["lines"][sku]):
            line["quarantined"] -= accepted + rejected
            line["accepted"] += accepted
            line["rejected"] += rejected
        if accepted:
            receive_stock(state, settings, context.tenant, row["warehouse"], sku, accepted,
                          f"{receipt_id}:inspection:{row['version']}")
    inspection = {"actor": context.actor, "reason": reason,
                  "lines": [{"sku": sku, "accepted": accepted, "rejected": rejected}
                            for sku, accepted, rejected in prepared]}
    row["inspections"].append(inspection)
    row["version"] += 1
    order["version"] += 1
    refresh_status(order)
    allocations = allocate_shortages(state, settings, context.tenant) if allocate else []
    row["allocations"].extend(allocations)
    record(state, context, "receipt.inspected", receipt_id, inspection)
    return receipt_view(row)


def receipt_view(row):
    result = snapshot(row)
    result.pop("request")
    result["accepted_cents"] = sum(line["accepted"] * line["unit_cents"] for line in row["lines"].values())
    result["quarantine_quantity"] = sum(line["quarantined"] for line in row["lines"].values())
    return result
