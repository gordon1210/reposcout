from meridian.core.errors import require
from meridian.core.quantities import quantity
from meridian.inventory.repository import reservation_rows


def shipment_plan(state, tenant, order_id, sku, units, warehouse=None):
    remaining = quantity(units)
    plan = []
    for row in reservation_rows(state, tenant, order_id, sku):
        if warehouse is not None and row.warehouse != warehouse:
            continue
        selected = min(row.reserved, remaining)
        if selected:
            plan.append((row, selected))
            remaining -= selected
        if remaining == 0:
            break
    require(remaining == 0, "insufficient_reservation", "Requested units are not reserved at this warehouse", 409)
    return plan
