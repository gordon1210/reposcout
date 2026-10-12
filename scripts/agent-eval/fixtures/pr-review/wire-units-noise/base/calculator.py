def total_cents(items):
    return sum(item["quantity"] * item["unit_price_cents"] for item in items)
