from meridian.orders.repository import list_orders
from meridian.orders.totals import order_totals


def sales_report(state, tenant):
    by_sku = {}
    net_cents = 0
    orders = list_orders(state, tenant)
    for order in orders:
        net_cents += order_totals(order)["net_cents"]
        for sku, line in order.lines.items():
            row = by_sku.setdefault(sku, {"sku": sku, "ordered": 0, "shipped": 0, "cancelled": 0, "returned": 0})
            for key in ("ordered", "shipped", "cancelled", "returned"):
                row[key] += getattr(line, key)
    return {"order_count": len(orders), "net_cents": net_cents, "currency": "EUR",
            "products": [row for _, row in sorted(by_sku.items())]}
