"""Validation and tenant-scoped lookup shared by logistics commands."""
from copy import deepcopy
from datetime import date
from meridian.core.errors import require


def text(value, name, limit=120):
    require(isinstance(value, str) and 0 < len(value.strip()) <= limit,
            "invalid_logistics_text", f"{name} must contain at most {limit} characters")
    return value.strip()


def integer(value, name, minimum=0, maximum=1000000):
    require(type(value) is int and minimum <= value <= maximum,
            "invalid_logistics_integer", f"{name} must be an integer from {minimum} to {maximum}")
    return value


def boolean(value, name):
    require(type(value) is bool, "invalid_logistics_boolean", f"{name} must be boolean")
    return value


def day(value):
    require(isinstance(value, str), "invalid_shipping_date", "Shipping date must be ISO YYYY-MM-DD")
    try:
        parsed = date.fromisoformat(value)
    except ValueError:
        require(False, "invalid_shipping_date", "Shipping date must be ISO YYYY-MM-DD")
    require(parsed.isoformat() == value, "invalid_shipping_date", "Shipping date must be ISO YYYY-MM-DD")
    return value


def lookup(collection, tenant, identity, kind):
    require(isinstance(identity, str), f"{kind}_not_found", f"Unknown {kind}", 404)
    row = collection.get((tenant, identity))
    require(row is not None, f"{kind}_not_found", f"Unknown {kind}", 404)
    return row


def warehouse(settings, tenant, value):
    require(value in settings.warehouses.get(tenant, ()), "warehouse_not_found", "Unknown warehouse", 404)
    return value


def public(row):
    return deepcopy({key: value for key, value in row.items() if key != "tenant"})


def tick(value):
    return integer(value, "tick", maximum=1000000000)


def items(value):
    require(isinstance(value, list) and 1 <= len(value) <= 100,
            "invalid_logistics_items", "Provide between one and one hundred items")
    result = {}
    for entry in value:
        require(isinstance(entry, dict), "invalid_logistics_item", "Each item must be an object")
        sku = text(entry.get("sku"), "sku", 80)
        require(sku not in result, "duplicate_logistics_item", "Each SKU must occur once")
        result[sku] = integer(entry.get("quantity"), "quantity", 1, 10000)
    return result
