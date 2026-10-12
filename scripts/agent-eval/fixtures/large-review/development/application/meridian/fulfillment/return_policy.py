from meridian.core.errors import require
from meridian.core.quantities import bounded_quantity

REASONS = {"unwanted", "wrong_item", "damaged"}


def return_quantity(state, tenant, shipment, requested, days, reason):
    require(type(days) is int and 0 <= days <= 30, "return_window", "Returns are accepted through day 30")
    require(reason in REASONS, "invalid_return_reason", "Choose an accepted return reason")
    returned = sum(row["quantity"] for (owner, _), row in state.returns.items()
                   if owner == tenant and row["shipment_id"] == shipment["shipment_id"])
    return bounded_quantity(requested, shipment["quantity"] - returned)
