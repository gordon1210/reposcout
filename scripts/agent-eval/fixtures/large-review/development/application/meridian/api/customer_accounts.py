from meridian.customers.accounts import get_customer, change_address
from meridian.customers.preferences import update_preferences
from meridian.billing.statements import customer_statement


def profile(app, context, body, params):
    return get_customer(app.state, context.tenant, context.actor).public()


def address(app, context, body, params):
    return change_address(app.state, context.tenant, context.actor, body).public()


def preferences(app, context, body, params):
    return update_preferences(app.state, context.tenant, context.actor, body)


def statement(app, context, body, params):
    return customer_statement(app.state, context.tenant, context.actor)
