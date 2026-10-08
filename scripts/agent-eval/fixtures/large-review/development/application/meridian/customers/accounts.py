from meridian.core.errors import require
from meridian.core.identity import identifier
from meridian.customers.address import parse_address
from meridian.customers.profile import Customer


def get_customer(state, tenant, customer_id):
    result = state.customers.get((tenant, customer_id))
    require(result is not None, "customer_not_found", "Customer does not exist", 404)
    return result


def register_customer(state, tenant, body):
    customer_id = identifier(body.get("customer_id"), "customer_id")
    require((tenant, customer_id) not in state.customers, "customer_exists", "Customer already exists", 409)
    email = body.get("email", "")
    require(isinstance(email, str) and email.count("@") == 1 and len(email) <= 254,
            "invalid_email", "Provide an email address")
    customer = Customer(tenant, customer_id, email.lower(), parse_address(body.get("address")))
    state.customers[(tenant, customer_id)] = customer
    return customer


def change_address(state, tenant, customer_id, body):
    customer = get_customer(state, tenant, customer_id)
    customer.address = parse_address(body)
    return customer
