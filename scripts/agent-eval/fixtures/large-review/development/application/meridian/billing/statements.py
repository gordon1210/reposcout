from meridian.billing.invoices import invoice_view
from meridian.orders.repository import list_orders


def customer_statement(state, tenant, customer_id):
    invoices = [invoice_view(state, tenant, order.order_id)
                for order in list_orders(state, tenant, customer_id)]
    invoices.sort(key=lambda row: row["invoice_id"])
    return {"customer_id": customer_id, "currency": "EUR", "invoices": invoices,
            "due_cents": sum(row["due_cents"] for row in invoices),
            "refundable_cents": sum(row["refundable_cents"] for row in invoices)}
