from meridian.core.errors import require
from meridian.orders.repository import get_line
from meridian.orders.totals import line_value
from meridian.fulfillment.return_policy import return_quantity
from meridian.fulfillment.return_plan import return_allocations
from meridian.inventory.repository import stock_bin
from meridian.inventory.ledger import post_inventory
from meridian.billing.credits import issue_credit
from meridian.events.outbox import publish
from meridian.core.audit import record


def accept_return(state, settings, context, order, body):
    shipment = state.shipments.get((context.tenant, body.get("shipment_id")))
    require(shipment is not None and shipment["order_id"] == order.order_id,
            "shipment_not_found", "Shipment is not on this order", 404)
    reason = body.get("reason")
    count = return_quantity(state, context.tenant, shipment, body.get("quantity"), body.get("days"), reason)
    line = get_line(order, shipment["sku"])
    allocations = return_allocations(state, context.tenant, shipment, count)
    return_id = state.next_id(context.tenant, "return")
    restock = reason != "damaged"
    before_credit = line_value(line, line.returned)
    line.returned += count
    for allocation in allocations:
        units = allocation["quantity"] if restock else 0
        stock = stock_bin(state, settings, context.tenant, allocation["warehouse"], line.sku)
        stock.on_hand += units
        post_inventory(state, context.tenant, "return", allocation["warehouse"], line.sku,
                       units, 0, order.order_id, return_id)
    credit_cents = line_value(line, line.returned) - before_credit
    credit = issue_credit(state, context.tenant, order.order_id, credit_cents, "return")
    entry = {"return_id": return_id, "order_id": order.order_id, "shipment_id": shipment["shipment_id"],
             "sku": line.sku, "quantity": count, "reason": reason, "restocked": restock,
             "allocations": allocations, "credit_id": credit["credit_id"], "credit_cents": credit_cents}
    state.returns[(context.tenant, return_id)] = entry
    order.touch()
    publish(state, context.tenant, "return.accepted", {"order_id": order.order_id, "return_id": return_id})
    record(state, context, "order.return", order.order_id, {"quantity": count, "reason": reason})
    return entry
