from meridian.core.errors import require


def get_order(state, tenant, order_id):
    result = state.orders.get((tenant, order_id))
    require(result is not None, "order_not_found", "Order does not exist", 404)
    return result


def get_line(order, sku):
    result = order.lines.get(sku)
    require(result is not None, "line_not_found", "Order does not contain this SKU", 404)
    return result


def list_orders(state, tenant, customer_id=None):
    return [order for (owner, _), order in state.orders.items()
            if owner == tenant and (customer_id is None or order.customer_id == customer_id)]
