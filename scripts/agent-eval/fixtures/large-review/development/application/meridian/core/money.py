from decimal import Decimal, ROUND_HALF_UP
from meridian.core.errors import require


def cents(value, field="amount_cents", allow_zero=False):
    require(type(value) is int, "invalid_money", f"{field} must use integer cents")
    require(value >= (0 if allow_zero else 1), "invalid_money", f"{field} is negative or zero")
    return value


def ratio_round(amount, numerator, denominator):
    require(denominator > 0, "invalid_ratio", "The denominator must be positive")
    return int((Decimal(amount) * Decimal(numerator) / Decimal(denominator)).quantize(
        Decimal("1"), rounding=ROUND_HALF_UP))


def split_cents(total, weights):
    require(all(w >= 0 for w in weights) and sum(weights) > 0,
            "invalid_weights", "At least one positive weight is required")
    shares = [total * weight // sum(weights) for weight in weights]
    remainder = total - sum(shares)
    ranking = sorted(range(len(weights)), key=lambda i: (-(total * weights[i] % sum(weights)), i))
    for index in ranking[:remainder]:
        shares[index] += 1
    return shares
