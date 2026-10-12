"""Three-way invoice matching against approved cost and inspected receipt quantities."""
from meridian.core.errors import require
from meridian.core.audit import record
from meridian.purchasing.orders import purchase_order
from meridian.purchasing.validation import lookup, snapshot, text, unique_skus, integer, version


def create_invoice(state, context, order_id, body):
    order = purchase_order(state, context.tenant, order_id)
    require(order["status"] not in {"draft", "cancelled"}, "purchase_status", "Invoice requires an approved purchase", 409)
    reference = text(body.get("vendor_reference"), "vendor_reference", 80)
    lines = {}
    for sku, raw in unique_skus(body.get("lines")).items():
        require(sku in order["lines"], "purchase_line_missing", "Invoice contains an unordered SKU", 409)
        lines[sku] = {"sku": sku, "quantity": integer(raw.get("quantity"), "quantity", 1, 1_000_000),
                      "unit_cents": integer(raw.get("unit_cents"), "unit_cents", 1)}
    for (tenant, _), previous in state.purchase_invoices.items():
        if tenant == context.tenant and previous["supplier_id"] == order["supplier_id"] and previous["vendor_reference"] == reference:
            require(previous["purchase_order_id"] == order_id and previous["lines"] == lines,
                    "invoice_conflict", "Vendor invoice reference already has different content", 409)
            return invoice_view(state, previous)
    invoice_id = state.next_id(context.tenant, "vendor-invoice")
    invoice = {"invoice_id": invoice_id, "tenant": context.tenant, "purchase_order_id": order_id,
               "supplier_id": order["supplier_id"], "vendor_reference": reference,
               "lines": lines, "status": "pending", "version": 1, "match": None,
               "settlement_reference": None}
    state.purchase_invoices[(context.tenant, invoice_id)] = invoice
    record(state, context, "vendor_invoice.created", invoice_id)
    return invoice_view(state, invoice)


def evaluate_match(state, invoice):
    order = purchase_order(state, invoice["tenant"], invoice["purchase_order_id"])
    issues = []
    amounts = []
    for sku, line in sorted(invoice["lines"].items()):
        purchased = order["lines"][sku]
        committed = sum(other["lines"].get(sku, {}).get("quantity", 0)
                        for (tenant, other_id), other in state.purchase_invoices.items()
                        if tenant == invoice["tenant"] and other_id != invoice["invoice_id"]
                        and other["purchase_order_id"] == invoice["purchase_order_id"]
                        and other["status"] in {"matched", "settled"})
        available = purchased["accepted"] - committed
        if line["quantity"] > available:
            issues.append({"sku": sku, "kind": "quantity", "invoiced": line["quantity"],
                           "accepted_available": max(0, available)})
        if line["unit_cents"] != purchased["unit_cents"]:
            issues.append({"sku": sku, "kind": "price", "invoiced_unit_cents": line["unit_cents"],
                           "agreed_unit_cents": purchased["unit_cents"]})
        amounts.append({"sku": sku, "quantity": line["quantity"],
                        "agreed_cents": line["quantity"] * purchased["unit_cents"],
                        "invoiced_cents": line["quantity"] * line["unit_cents"]})
    return {"matched": not issues, "issues": issues, "lines": amounts,
            "purchase_version": order["version"]}


def match_invoice(state, context, invoice_id, body):
    invoice = lookup(state.purchase_invoices, context.tenant, invoice_id, "vendor_invoice")
    version(invoice, body.get("expected_version"))
    require(invoice["status"] in {"pending", "disputed"}, "invoice_status", "Invoice has already been matched", 409)
    result = evaluate_match(state, invoice)
    invoice.update(match=result, status="matched" if result["matched"] else "disputed",
                   version=invoice["version"] + 1)
    record(state, context, "vendor_invoice.matched", invoice_id, {"matched": result["matched"]})
    return invoice_view(state, invoice)


def correct_invoice(state, context, invoice_id, body):
    invoice = lookup(state.purchase_invoices, context.tenant, invoice_id, "vendor_invoice")
    version(invoice, body.get("expected_version"))
    require(invoice["status"] in {"pending", "disputed"}, "invoice_status", "Matched invoices cannot be edited", 409)
    reason = text(body.get("reason"), "reason")
    order = purchase_order(state, context.tenant, invoice["purchase_order_id"])
    lines = {}
    for sku, raw in unique_skus(body.get("lines")).items():
        require(sku in order["lines"], "purchase_line_missing", "Invoice contains an unordered SKU", 409)
        lines[sku] = {"sku": sku, "quantity": integer(raw.get("quantity"), "quantity", 1, 1_000_000),
                      "unit_cents": integer(raw.get("unit_cents"), "unit_cents", 1)}
    invoice.update(lines=lines, status="pending", match=None, version=invoice["version"] + 1)
    record(state, context, "vendor_invoice.corrected", invoice_id, {"reason": reason})
    return invoice_view(state, invoice)


def settle(state, context, invoice_id, body):
    invoice = lookup(state.purchase_invoices, context.tenant, invoice_id, "vendor_invoice")
    version(invoice, body.get("expected_version"))
    require(invoice["status"] == "matched", "invoice_status", "Only matched invoices can be settled", 409)
    reference = text(body.get("settlement_reference"), "settlement_reference", 80)
    amount = integer(body.get("amount_cents"), "amount_cents")
    require(amount == invoice_view(state, invoice)["payable_cents"], "settlement_amount", "Settlement must equal the payable amount", 409)
    require(not any(tenant == context.tenant and other["settlement_reference"] == reference
                    for (tenant, _), other in state.purchase_invoices.items()),
            "settlement_conflict", "Settlement reference already used", 409)
    invoice.update(status="settled", settlement_reference=reference, settled_cents=amount,
                   version=invoice["version"] + 1)
    record(state, context, "vendor_invoice.settled", invoice_id, {"amount_cents": amount, "reference": reference})
    return invoice_view(state, invoice)


def invoice_view(state, invoice):
    result = snapshot(invoice)
    result["total_cents"] = sum(line["quantity"] * line["unit_cents"] for line in invoice["lines"].values())
    result["credited_cents"] = sum(row["amount_cents"] for (tenant, _), row in state.purchase_credits.items()
                                   if tenant == invoice["tenant"] and row.get("invoice_id") == invoice["invoice_id"])
    result["payable_cents"] = result["total_cents"] - result["credited_cents"]
    return result
