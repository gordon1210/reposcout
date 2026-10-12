from meridian.core.quantities import bounded_quantity
from meridian.inventory.availability import reserved_for_order


def cancellable_quantity(state, order, line, requested):
    remaining = min(line.outstanding,
                    reserved_for_order(state, order.tenant, order.order_id, line.sku))
    return bounded_quantity(requested, remaining, "quantity", allow_zero=True)
