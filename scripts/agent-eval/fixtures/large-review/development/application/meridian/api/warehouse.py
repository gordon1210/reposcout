from meridian.inventory.adjustments import receive_stock, count_stock
from meridian.inventory.availability import stock_view
from meridian.inventory.transfers import transfer_stock
from meridian.inventory.reservations import reserve_order
from meridian.inventory.ledger import inventory_entries
from meridian.orders.repository import get_order
from meridian.fulfillment.shipments import ship_units
from meridian.fulfillment.picklists import warehouse_picklist
from meridian.api.presenters import order_view


def stock(app, context, body, params):
    return stock_view(app.state, app.settings, context.tenant, params["sku"])


def receive(app, context, body, params):
    row = receive_stock(app.state, app.settings, context.tenant, body.get("warehouse"),
                        body.get("sku"), body.get("quantity"), body.get("reference"))
    return stock_view(app.state, app.settings, context.tenant, row.sku)


def count(app, context, body, params):
    row = count_stock(app.state, app.settings, context.tenant, body.get("warehouse"),
                      body.get("sku"), body.get("counted"), body.get("reason"))
    return stock_view(app.state, app.settings, context.tenant, row.sku)


def transfer(app, context, body, params):
    return transfer_stock(app.state, app.settings, context.tenant, body.get("sku"),
                          body.get("source"), body.get("destination"), body.get("quantity"))


def allocate(app, context, body, params):
    order = get_order(app.state, context.tenant, params["order_id"])
    reserve_order(app.state, app.settings, order, allow_partial=True)
    return order_view(app.state, order)


def ship(app, context, body, params):
    return ship_units(app.state, app.settings, context, params["order_id"], body)


def picklist(app, context, body, params):
    return {"orders": warehouse_picklist(app.state, context.tenant, params["warehouse"])}


def ledger(app, context, body, params):
    return {"entries": inventory_entries(app.state, context.tenant, body.get("order_id"), body.get("sku"), body.get("kind"))}
