from meridian.baskets.model import Basket
from meridian.baskets.repository import get_basket, verify_version
from meridian.catalog.repository import get_product
from meridian.catalog.visibility import require_visible
from meridian.customers.accounts import get_customer
from meridian.core.quantities import quantity


def create_basket(state, context):
    get_customer(state, context.tenant, context.actor)
    basket = Basket(context.tenant, state.next_id(context.tenant, "basket"), context.actor)
    state.baskets[(context.tenant, basket.basket_id)] = basket
    return basket


def set_item(state, context, basket_id, body):
    basket = get_basket(state, context.tenant, context.actor, basket_id, editable=True)
    verify_version(basket, body.get("version"))
    customer = get_customer(state, context.tenant, context.actor)
    product = require_visible(get_product(state, context.tenant, body.get("sku")), customer)
    count = quantity(body.get("quantity"), allow_zero=True)
    if count:
        basket.items[product.sku] = count
    else:
        basket.items.pop(product.sku, None)
    basket.version += 1
    return basket
