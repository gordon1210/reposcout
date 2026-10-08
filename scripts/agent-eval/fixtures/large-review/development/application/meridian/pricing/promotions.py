from meridian.core.errors import require


def discount_for(code, subtotal, customer):
    if code is None or code == "":
        return 0
    require(isinstance(code, str), "invalid_promotion", "Promotion code must be text")
    if code == "WELCOME" and subtotal >= 2000 and customer.completed_orders == 0:
        return 500
    if code == "TRADE10" and customer.wholesale and subtotal >= 10000:
        return subtotal // 10
    return 0


def apply_discount(lines, discount):
    from meridian.core.money import split_cents
    if not discount:
        return [dict(line, discount_cents=0) for line in lines]
    shares = split_cents(discount, [line["subtotal_cents"] for line in lines])
    return [dict(line, discount_cents=share) for line, share in zip(lines, shares)]
