from meridian.baskets.commands import create_basket, set_item
from meridian.baskets.repository import get_basket
from meridian.baskets.checkout import quote_basket, checkout_basket
from meridian.api.presenters import order_view


def create(app, context, body, params):
    return create_basket(app.state, context).public()


def show(app, context, body, params):
    return get_basket(app.state, context.tenant, context.actor, params["basket_id"]).public()


def update_item(app, context, body, params):
    return set_item(app.state, context, params["basket_id"], body).public()


def quote(app, context, body, params):
    return quote_basket(app, context, params["basket_id"], body)


def checkout(app, context, body, params):
    return order_view(app.state, checkout_basket(app, context, params["basket_id"], body))
