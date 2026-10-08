from meridian.core.errors import require


def quantity(value, field="quantity", allow_zero=False):
    require(type(value) is int, "invalid_quantity", f"{field} must be an integer")
    minimum = 0 if allow_zero else 1
    require(value >= minimum, "invalid_quantity", f"{field} must be at least {minimum}")
    require(value <= 1_000_000, "invalid_quantity", f"{field} exceeds the per-request limit")
    return value


def bounded_quantity(value, maximum, field="quantity", allow_zero=False):
    result = quantity(value, field, allow_zero)
    require(result <= maximum, "quantity_exceeds_remaining", f"{field} exceeds {maximum}", 409)
    return result
