from meridian.core.errors import require
from meridian.customers.accounts import get_customer

ALLOWED = {"email_receipts", "language"}


def update_preferences(state, tenant, customer_id, body):
    require(set(body) <= ALLOWED, "invalid_preference", "Unknown preference")
    if "email_receipts" in body:
        require(type(body["email_receipts"]) is bool, "invalid_preference", "email_receipts must be boolean")
    if "language" in body:
        require(body["language"] in ("en", "de", "fr"), "invalid_preference", "Unsupported language")
    customer = get_customer(state, tenant, customer_id)
    customer.preferences.update(body)
    return dict(customer.preferences)
