from meridian.catalog.search import search_catalog
from meridian.catalog.repository import get_product
from meridian.catalog.visibility import require_visible
from meridian.catalog.bundles import expand_components, bundle_capacity
from meridian.inventory.availability import stock_view
from meridian.customers.accounts import get_customer
from meridian.pricing.quote import quote


def listing(app, context, body, params):
    customer = get_customer(app.state, context.tenant, context.actor)
    return search_catalog(app.state, context.tenant, customer, body)


def detail(app, context, body, params):
    customer = get_customer(app.state, context.tenant, context.actor)
    product = require_visible(get_product(app.state, context.tenant, params["sku"]), customer)
    available = lambda sku: stock_view(app.state, app.settings, context.tenant, sku)["available"]
    view = dict(product.public(), available=bundle_capacity(app.state, context.tenant, product.sku, available))
    if product.components:
        view["components"] = expand_components(app.state, context.tenant, product.sku)
    return view


def price(app, context, body, params):
    customer = get_customer(app.state, context.tenant, context.actor)
    return quote(app.state, app.settings, context.tenant, customer, body.get("items"), body)
