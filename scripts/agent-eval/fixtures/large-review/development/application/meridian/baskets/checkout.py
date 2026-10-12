from meridian.baskets.repository import get_basket, verify_version
from meridian.orders.creation import create_order
from meridian.orders.repository import get_order
from meridian.pricing.quote import quote
from meridian.customers.accounts import get_customer


def quote_basket(app, context, basket_id, options):
    basket = get_basket(app.state, context.tenant, context.actor, basket_id)
    customer = get_customer(app.state, context.tenant, context.actor)
    return quote(app.state, app.settings, context.tenant, customer, basket.public()["items"], options)


def checkout_basket(app, context, basket_id, body):
    basket = get_basket(app.state, context.tenant, context.actor, basket_id)
    if basket.checked_out_order:
        return get_order(app.state, context.tenant, basket.checked_out_order)
    verify_version(basket, body.get("version"))
    order = create_order(app, context, dict(body, items=basket.public()["items"]))
    basket.checked_out_order = order.order_id
    basket.version += 1
    return order
