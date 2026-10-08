from meridian.core.errors import require
from meridian.inventory.availability import available_in_bin


def allocation_plan(state, settings, tenant, sku, units, allow_partial):
    remaining = units
    plan = []
    for warehouse in settings.warehouses[tenant]:
        available = available_in_bin(state, settings, tenant, warehouse, sku)
        selected = min(remaining, available)
        if selected > 0:
            plan.append((warehouse, selected))
            remaining -= selected
        if not remaining:
            break
    require(allow_partial or remaining == 0, "insufficient_stock", "Not enough stock to reserve", 409)
    return plan
