from meridian.core.errors import require
from meridian.core.pagination import page
from meridian.orders.repository import get_order, list_orders


def owned_order(state, tenant, customer_id, order_id):
    order = get_order(state, tenant, order_id)
    require(order.customer_id == customer_id, "order_not_found", "Order does not exist", 404)
    return order


def customer_page(state, tenant, customer_id, request):
    return page(list_orders(state, tenant, customer_id), request, lambda order: order.order_id)


def backorders(state, tenant, reserved_for):
    result = []
    for order in list_orders(state, tenant):
        missing = {sku: line.outstanding - reserved_for(order.order_id, sku)
                   for sku, line in order.lines.items()}
        missing = {sku: units for sku, units in missing.items() if units > 0}
        if missing:
            result.append({"order_id": order.order_id, "missing": missing})
    return sorted(result, key=lambda row: row["order_id"])
