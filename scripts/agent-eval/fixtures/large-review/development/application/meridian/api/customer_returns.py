from meridian.orders.queries import owned_order
from meridian.fulfillment.returns import accept_return
from meridian.api.presenters import order_view
from meridian.billing.credits import order_credits


def create(app, context, body, params):
    order = owned_order(app.state, context.tenant, context.actor, params["order_id"])
    entry = accept_return(app.state, app.settings, context, order, body)
    return dict(entry, order=order_view(app.state, order))


def credits(app, context, body, params):
    order = owned_order(app.state, context.tenant, context.actor, params["order_id"])
    return {"credits": order_credits(app.state, context.tenant, order.order_id)}
