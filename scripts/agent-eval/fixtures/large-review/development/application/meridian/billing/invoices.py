from meridian.core.errors import require
from meridian.orders.totals import order_totals


def create_invoice(state, order):
    invoice = {"invoice_id": state.next_id(order.tenant, "invoice"), "order_id": order.order_id,
               "currency": "EUR", "total_cents": order_totals(order)["original_cents"]}
    state.invoices[(order.tenant, order.order_id)] = invoice
    return invoice


def get_invoice(state, tenant, order_id):
    invoice = state.invoices.get((tenant, order_id))
    require(invoice is not None, "invoice_not_found", "Invoice does not exist", 404)
    return invoice


def invoice_view(state, tenant, order_id):
    invoice = get_invoice(state, tenant, order_id)
    captured = sum(row["amount_cents"] for (owner, _), row in state.payments.items()
                   if owner == tenant and row["order_id"] == order_id)
    credited = sum(row["amount_cents"] for (owner, _), row in state.credits.items()
                   if owner == tenant and row["order_id"] == order_id)
    return dict(invoice, captured_cents=captured, credited_cents=credited,
                due_cents=max(0, invoice["total_cents"] - credited - captured),
                refundable_cents=max(0, captured - (invoice["total_cents"] - credited)))
