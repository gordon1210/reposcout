"""Read-only receiving control totals with inventory-ledger provenance."""
from meridian.purchasing.orders import purchase_order, order_view
from meridian.purchasing.validation import snapshot


def reconcile_purchase(state, tenant, order_id):
    order = purchase_order(state, tenant, order_id)
    receipts = [row for (owner, _), row in state.purchase_receipts.items()
                if owner == tenant and row["purchase_order_id"] == order_id]
    receipt_ids = {row["receipt_id"] for row in receipts}
    evidence = []
    for sku, purchased in sorted(order["lines"].items()):
        totals = {field: sum(row["lines"].get(sku, {}).get(field, 0) for row in receipts)
                  for field in ("accepted", "quarantined", "rejected", "received", "credited_quantity")}
        entries = [row for row in state.inventory_ledger if row["tenant"] == tenant and row["sku"] == sku
                   and row["kind"] == "receive" and row["warehouse"] == order["warehouse"]
                   and isinstance(row.get("reference"), str)
                   and row["reference"].split(":inspection:", 1)[0] in receipt_ids]
        ledger_units = sum(row["on_hand_delta"] for row in entries)
        issues = []
        if totals["received"] != totals["accepted"] + totals["quarantined"] + totals["rejected"]:
            issues.append("receipt_disposition_balance")
        for field in ("accepted", "quarantined", "rejected", "received"):
            if totals[field] != purchased[field]:
                issues.append(f"purchase_{field}_balance")
        if ledger_units != totals["accepted"]:
            issues.append("inventory_receipt_balance")
        evidence.append({"sku": sku, **totals, "ledger_received": ledger_units,
                         "ledger_entry_ids": [row["entry_id"] for row in entries], "issues": issues})
    invoices = [row for (owner, _), row in state.purchase_invoices.items()
                if owner == tenant and row["purchase_order_id"] == order_id]
    return {"purchase_order": order_view(order), "lines": evidence,
            "balanced": all(not row["issues"] for row in evidence),
            "receipt_ids": sorted(receipt_ids),
            "invoice_statuses": [{"invoice_id": row["invoice_id"], "status": row["status"]} for row in invoices]}


def supplier_statement(state, tenant, supplier_id):
    from meridian.purchasing.suppliers import supplier
    from meridian.purchasing.matching import invoice_view
    vendor = supplier(state, tenant, supplier_id)
    invoices = [invoice_view(state, row) for (owner, _), row in state.purchase_invoices.items()
                if owner == tenant and row["supplier_id"] == supplier_id]
    credits = [snapshot(row) for (owner, _), row in state.purchase_credits.items()
               if owner == tenant and row["supplier_id"] == supplier_id]
    return {"supplier_id": supplier_id, "currency": vendor["currency"], "invoices": invoices,
            "credits": credits, "payable_cents": sum(row["payable_cents"] for row in invoices if row["status"] == "matched"),
            "disputed_cents": sum(row["total_cents"] for row in invoices if row["status"] == "disputed"),
            "settled_cents": sum(row.get("settled_cents", 0) for row in invoices),
            "unapplied_credit_cents": sum(row["amount_cents"] for row in credits if row["invoice_id"] is None)}
