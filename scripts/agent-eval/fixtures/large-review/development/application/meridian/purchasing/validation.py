"""Boundary rules shared by purchasing commands, independent of HTTP transport."""
from copy import deepcopy
from meridian.core.errors import require
from meridian.core.identity import identifier
from meridian.core.quantities import quantity


def integer(value, field, minimum=0, maximum=100_000_000):
    require(type(value) is int and minimum <= value <= maximum,
            "invalid_integer", f"{field} must be an integer between {minimum} and {maximum}")
    return value


def text(value, field, limit=200):
    require(isinstance(value, str) and bool(value.strip()) and len(value) <= limit,
            "invalid_text", f"{field} must be nonempty text of at most {limit} characters")
    return value.strip()


def boolean(value, field):
    require(type(value) is bool, "invalid_boolean", f"{field} must be boolean")
    return value


def rows(value, field="lines", limit=100):
    require(isinstance(value, list) and 0 < len(value) <= limit,
            "invalid_lines", f"{field} must contain between one and {limit} rows")
    require(all(isinstance(row, dict) for row in value), "invalid_lines", "Each row must be an object")
    return value


def unique_skus(value):
    result = {}
    for row in rows(value):
        sku = identifier(row.get("sku"), "sku")
        require(sku not in result, "duplicate_sku", "Each SKU may appear only once")
        result[sku] = row
    return result


def warehouse(settings, tenant, value):
    value = identifier(value, "warehouse")
    require(value in settings.warehouses.get(tenant, ()), "warehouse_not_found", "Unknown warehouse", 404)
    return value


def lookup(table, tenant, value, kind):
    key = identifier(value, kind)
    result = table.get((tenant, key))
    require(result is not None, f"{kind}_not_found", f"Unknown {kind}", 404)
    return result


def version(row, value):
    expected = integer(value, "expected_version", 1)
    require(row["version"] == expected, "stale_version", "Record changed; reload before updating", 409)


def snapshot(row):
    return deepcopy(row)


def pack_quantity(units, minimum, pack):
    units = quantity(units)
    require(units >= minimum and units % pack == 0,
            "invalid_pack_quantity", "Quantity must meet the minimum and be a pack multiple")
    return units
