from meridian.core.errors import require
from meridian.orders.repository import get_order, get_line
from meridian.inventory.repository import stock_bin
from meridian.inventory.ledger import post_inventory
from meridian.fulfillment.shipment_plan import shipment_plan
from meridian.events.outbox import publish
from meridian.core.audit import record


def ship_units(state, settings, context, order_id, body):
    order = get_order(state, context.tenant, order_id)
    line = get_line(order, body.get("sku"))
    plan = shipment_plan(state, context.tenant, order_id, line.sku,
                         body.get("quantity"), body.get("warehouse"))
    shipment_id = state.next_id(context.tenant, "shipment")
    allocations = []
    for reservation, units in plan:
        stock = stock_bin(state, settings, context.tenant, reservation.warehouse, line.sku)
        require(stock.on_hand >= units, "stock_changed", "Physical stock no longer covers shipment", 409)
        stock.on_hand -= units
        reservation.reserved -= units
        reservation.shipped += units
        line.shipped += units
        allocations.append({"warehouse": reservation.warehouse, "quantity": units})
        post_inventory(state, context.tenant, "ship", reservation.warehouse, line.sku,
                       -units, -units, order_id, shipment_id)
    shipment = {"shipment_id": shipment_id, "order_id": order_id, "sku": line.sku,
                "quantity": sum(row["quantity"] for row in allocations), "allocations": allocations,
                "tracking": [], "delivered": False}
    state.shipments[(context.tenant, shipment_id)] = shipment
    order.touch()
    publish(state, context.tenant, "shipment.created", {"order_id": order_id, "shipment_id": shipment_id})
    record(state, context, "order.ship", order_id, {"shipment_id": shipment_id})
    return shipment
