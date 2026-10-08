"""Purchase orders freeze agreed prices and packs at approval."""
from meridian.core.errors import require
from meridian.core.audit import record
from meridian.events.outbox import publish
from meridian.purchasing.suppliers import supplier, terms_for
from meridian.purchasing.validation import (lookup, snapshot, unique_skus, warehouse,
                                          pack_quantity, version, text, integer)


def purchase_order(state, tenant, order_id):
    return lookup(state.purchase_orders, tenant, order_id, "purchase_order")


def draft_lines(state, tenant, supplier_id, items):
    result = {}
    for sku, raw in unique_skus(items).items():
        terms = terms_for(state, tenant, supplier_id, sku)
        units = pack_quantity(raw.get("quantity"), terms["minimum_quantity"], terms["pack_size"])
        result[sku] = {"sku": sku, "ordered": units, "unit_cents": terms["unit_cents"],
                       "terms_version": terms["version"], "pack_size": terms["pack_size"],
                       "minimum_quantity": terms["minimum_quantity"], "lead_days": terms["lead_days"],
                       "supplier_sku": terms["supplier_sku"], "accepted": 0, "quarantined": 0,
                       "rejected": 0, "received": 0, "closed_quantity": 0}
    return result


def create_draft(state, settings, context, body):
    vendor = supplier(state, context.tenant, body.get("supplier_id"), active=True)
    destination = warehouse(settings, context.tenant, body.get("warehouse"))
    lines = draft_lines(state, context.tenant, vendor["supplier_id"], body.get("lines"))
    order_id = state.next_id(context.tenant, "purchase")
    row = {"purchase_order_id": order_id, "tenant": context.tenant,
           "supplier_id": vendor["supplier_id"], "warehouse": destination,
           "lines": lines, "status": "draft", "version": 1, "created_by": context.actor,
           "approved_by": None, "approval_reference": None,
           "payment_days": vendor["payment_days"], "currency": vendor["currency"],
           "closed_reason": None, "source_plan_id": None}
    state.purchase_orders[(context.tenant, order_id)] = row
    record(state, context, "purchase.drafted", order_id)
    return order_view(row)


def amend_draft(state, settings, context, order_id, body):
    row = purchase_order(state, context.tenant, order_id)
    version(row, body.get("expected_version"))
    require(row["status"] == "draft", "purchase_status", "Only drafts may be amended", 409)
    destination = warehouse(settings, context.tenant, body.get("warehouse", row["warehouse"]))
    lines = draft_lines(state, context.tenant, row["supplier_id"], body.get("lines"))
    row.update(lines=lines, warehouse=destination, version=row["version"] + 1)
    record(state, context, "purchase.amended", order_id)
    return order_view(row)


def approve(state, context, order_id, body):
    row = purchase_order(state, context.tenant, order_id)
    version(row, body.get("expected_version"))
    require(row["status"] == "draft", "purchase_status", "Only drafts may be approved", 409)
    supplier(state, context.tenant, row["supplier_id"], active=True)
    reference = text(body.get("approval_reference"), "approval_reference")
    limit = integer(body.get("authorized_cents"), "authorized_cents")
    require(order_view(row)["total_cents"] <= limit, "approval_limit", "Order exceeds authorized amount", 409)
    for sku, line in row["lines"].items():
        current = terms_for(state, context.tenant, row["supplier_id"], sku)
        require(current["version"] == line["terms_version"], "terms_changed",
                "Supplier terms changed; amend the draft before approval", 409)
    row.update(status="approved", approved_by=context.actor, approval_reference=reference,
               version=row["version"] + 1)
    publish(state, context.tenant, "purchase.approved", {"purchase_order_id": order_id,
            "supplier_id": row["supplier_id"], "total_cents": order_view(row)["total_cents"]})
    record(state, context, "purchase.approved", order_id, {"reference": reference})
    return order_view(row)


def remaining(line):
    return line["ordered"] - line["received"] - line["closed_quantity"]


def refresh_status(row):
    if row["status"] in {"draft", "cancelled", "closed"}:
        return
    if all(remaining(line) == 0 for line in row["lines"].values()):
        row["status"] = "quarantined" if any(line["quarantined"] for line in row["lines"].values()) else "received"
    elif any(line["received"] for line in row["lines"].values()):
        row["status"] = "partially_received"
    else:
        row["status"] = "approved"


def close(state, context, order_id, body):
    row = purchase_order(state, context.tenant, order_id)
    version(row, body.get("expected_version"))
    require(row["status"] not in {"closed", "cancelled"}, "purchase_status", "Purchase is already closed", 409)
    require(not any(line["quarantined"] for line in row["lines"].values()),
            "quarantine_pending", "Resolve quarantined units before closing", 409)
    reason = text(body.get("reason"), "reason")
    for line in row["lines"].values():
        line["closed_quantity"] += remaining(line)
    row.update(status="cancelled" if row["status"] == "draft" else "closed",
               closed_reason=reason, version=row["version"] + 1)
    record(state, context, "purchase.closed", order_id, {"reason": reason})
    return order_view(row)


def order_view(row):
    result = snapshot(row)
    result["total_cents"] = sum(line["ordered"] * line["unit_cents"] for line in row["lines"].values())
    result["open_cents"] = sum(remaining(line) * line["unit_cents"] for line in row["lines"].values())
    for line in result["lines"].values():
        line["remaining"] = remaining(line)
    return result
