from meridian.billing.invoices import invoice_view
from meridian.billing.reconciliation import reconcile_billing
from meridian.orders.repository import list_orders
from meridian.events.outbox import outbox_summary


def finance_report(state, tenant):
    invoices = [invoice_view(state, tenant, order.order_id) for order in list_orders(state, tenant)]
    return {"invoiced_cents": sum(row["total_cents"] for row in invoices),
            "captured_cents": sum(row["captured_cents"] for row in invoices),
            "credited_cents": sum(row["credited_cents"] for row in invoices),
            "due_cents": sum(row["due_cents"] for row in invoices),
            "reconciliation": reconcile_billing(state, tenant), "outbox": outbox_summary(state, tenant)}
