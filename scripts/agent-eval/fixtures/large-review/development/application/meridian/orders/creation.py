from meridian.core.errors import require
from meridian.core.audit import record
from meridian.customers.accounts import get_customer
from meridian.pricing.quote import quote
from meridian.orders.model import Order, OrderLine
from meridian.inventory.reservations import reserve_order
from meridian.billing.invoices import create_invoice
from meridian.events.outbox import publish


def create_order(app, context, body):
    customer = get_customer(app.state, context.tenant, context.actor)
    allow_backorder = body.get("allow_backorder", False)
    require(type(allow_backorder) is bool, "invalid_backorder", "allow_backorder must be boolean")
    priced = quote(app.state, app.settings, context.tenant, customer, body.get("items"), body)
    order_id = app.state.next_id(context.tenant, "order")
    lines = {row["sku"]: OrderLine(row["sku"], row["quantity"], row["unit_cents"],
                                   row["discount_cents"], row["tax_cents"]) for row in priced["lines"]}
    order = Order(context.tenant, order_id, context.actor, lines, priced["shipping_cents"],
                  priced["region"], allow_backorder)
    app.state.orders[(context.tenant, order_id)] = order
    reserve_order(app.state, app.settings, order)
    create_invoice(app.state, order)
    customer.completed_orders += 1
    publish(app.state, context.tenant, "order.placed", {"order_id": order_id, "total_cents": priced["total_cents"]})
    record(app.state, context, "order.create", order_id, {"items": len(lines)})
    return order
