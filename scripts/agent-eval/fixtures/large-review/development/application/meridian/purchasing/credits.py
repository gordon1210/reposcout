"""Vendor credit notes record rejection claims separately from stock movements."""
from meridian.core.errors import require
from meridian.core.audit import record
from meridian.events.outbox import publish
from meridian.purchasing.validation import lookup, text, unique_skus, integer, snapshot
from meridian.purchasing.matching import invoice_view


def issue_credit(state, context, receipt_id, body):
    receipt = lookup(state.purchase_receipts, context.tenant, receipt_id, "receipt")
    reference = text(body.get("vendor_reference"), "vendor_reference", 80)
    reason = text(body.get("reason"), "reason")
    lines = {}
    for sku, raw in unique_skus(body.get("lines")).items():
        require(sku in receipt["lines"], "receipt_line_missing", "Credit SKU is not on the receipt", 409)
        units = integer(raw.get("quantity"), "quantity", 1, 1_000_000)
        line = receipt["lines"][sku]
        lines[sku] = {"sku": sku, "quantity": units, "unit_cents": line["unit_cents"],
                      "amount_cents": units * line["unit_cents"]}
    for (tenant, _), previous in state.purchase_credits.items():
        if tenant == context.tenant and previous["supplier_id"] == receipt["supplier_id"] and previous["vendor_reference"] == reference:
            require(previous["receipt_id"] == receipt_id and previous["lines"] == lines and previous["reason"] == reason,
                    "credit_conflict", "Credit reference has different content", 409)
            return snapshot(previous)
    for sku, line in lines.items():
        balance = receipt["lines"][sku]["rejected"] - receipt["lines"][sku]["credited_quantity"]
        require(line["quantity"] <= balance, "credit_exceeds_rejection", "Credit quantity exceeds uncredited rejected units", 409)
    credit_id = state.next_id(context.tenant, "vendor-credit")
    row = {"credit_id": credit_id, "tenant": context.tenant, "supplier_id": receipt["supplier_id"],
           "purchase_order_id": receipt["purchase_order_id"], "receipt_id": receipt_id,
           "vendor_reference": reference, "reason": reason, "lines": lines,
           "amount_cents": sum(line["amount_cents"] for line in lines.values()), "invoice_id": None}
    state.purchase_credits[(context.tenant, credit_id)] = row
    for sku, line in lines.items():
        receipt["lines"][sku]["credited_quantity"] += line["quantity"]
    receipt["version"] += 1
    record(state, context, "vendor_credit.issued", credit_id, {"receipt_id": receipt_id})
    publish(state, context.tenant, "vendor_credit.issued", {"credit_id": credit_id, "amount_cents": row["amount_cents"]})
    return snapshot(row)


def apply_credit(state, context, credit_id, body):
    credit = lookup(state.purchase_credits, context.tenant, credit_id, "vendor_credit")
    invoice = lookup(state.purchase_invoices, context.tenant, body.get("invoice_id"), "vendor_invoice")
    if credit["invoice_id"] is not None:
        require(credit["invoice_id"] == invoice["invoice_id"], "credit_already_applied", "Credit belongs to another invoice", 409)
        return snapshot(credit)
    require(invoice["supplier_id"] == credit["supplier_id"], "credit_supplier_mismatch", "Credit and invoice suppliers differ", 409)
    require(invoice["status"] == "matched", "invoice_status", "Apply credit to an unpaid matched invoice", 409)
    require(credit["amount_cents"] <= invoice_view(state, invoice)["payable_cents"],
            "credit_exceeds_invoice", "Credit exceeds the remaining invoice balance", 409)
    credit["invoice_id"] = invoice["invoice_id"]
    invoice["version"] += 1
    record(state, context, "vendor_credit.applied", credit_id, {"invoice_id": invoice["invoice_id"]})
    return snapshot(credit)
