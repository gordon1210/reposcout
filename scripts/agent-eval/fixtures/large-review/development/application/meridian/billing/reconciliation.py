from meridian.billing.invoices import invoice_view
from meridian.orders.repository import list_orders
from meridian.orders.totals import order_totals


def reconcile_billing(state, tenant):
    issues = []
    for order in list_orders(state, tenant):
        invoice = invoice_view(state, tenant, order.order_id)
        totals = order_totals(order)
        if invoice["total_cents"] != totals["original_cents"]:
            issues.append({"order_id": order.order_id, "kind": "invoice_total"})
        if invoice["credited_cents"] < totals["cancelled_cents"] + totals["returned_cents"]:
            issues.append({"order_id": order.order_id, "kind": "missing_credit"})
        if invoice["credited_cents"] > invoice["total_cents"]:
            issues.append({"order_id": order.order_id, "kind": "excess_credit"})
    return {"consistent": not issues, "issues": issues}
