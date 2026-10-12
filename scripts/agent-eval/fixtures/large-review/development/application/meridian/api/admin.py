from meridian.catalog.service import create_product, retire_product, reprice_product
from meridian.customers.accounts import register_customer
from meridian.orders.notes import add_note
from meridian.events.outbox import drain_outbox, outbox_summary
from meridian.events.inbox import receipt_summary
from meridian.core.audit import for_tenant
from meridian.api.validation import boolean_option


def product_create(app, context, body, params):
    return create_product(app.state, context.tenant, body).public()


def product_retire(app, context, body, params):
    return retire_product(app.state, context.tenant, params["sku"]).public()


def product_reprice(app, context, body, params):
    return reprice_product(app.state, context.tenant, params["sku"], body.get("unit_cents")).public()


def customer_create(app, context, body, params):
    return register_customer(app.state, context.tenant, body).public()


def internal_note(app, context, body, params):
    return add_note(app.state, context, params["order_id"], body.get("text"), internal=True)


def drain(app, context, body, params):
    return drain_outbox(app.state, context.tenant, body.get("tick", 0), boolean_option(body, "simulate_failure"))


def messages(app, context, body, params):
    return {"outbox": outbox_summary(app.state, context.tenant), "inbox": receipt_summary(app.state, context.tenant)}


def audit(app, context, body, params):
    return {"entries": for_tenant(app.state, context.tenant, body.get("action"))}
