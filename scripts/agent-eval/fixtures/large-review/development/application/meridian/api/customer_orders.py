from meridian.orders.creation import create_order
from meridian.orders.queries import owned_order, customer_page
from meridian.orders.notes import add_note
from meridian.fulfillment.cancellations import cancel_reserved_units
from meridian.api.presenters import order_view


def place(app, context, body, params):
    return order_view(app.state, create_order(app, context, body))


def show(app, context, body, params):
    order = owned_order(app.state, context.tenant, context.actor, params["order_id"])
    return order_view(app.state, order)


def listing(app, context, body, params):
    orders, cursor = customer_page(app.state, context.tenant, context.actor, body)
    return {"orders": [order_view(app.state, order) for order in orders], "next_cursor": cursor}


def cancel(app, context, body, params):
    order = owned_order(app.state, context.tenant, context.actor, params["order_id"])
    result = cancel_reserved_units(app.state, context, order, body.get("sku"), body.get("quantity"))
    return dict(result, order=order_view(app.state, order))


def note(app, context, body, params):
    owned_order(app.state, context.tenant, context.actor, params["order_id"])
    return add_note(app.state, context, params["order_id"], body.get("text"))
