from meridian.inventory.allocation import allocation_plan
from meridian.inventory.availability import reserved_for_order
from meridian.inventory.ledger import post_inventory
from meridian.inventory.model import Reservation


def reserve_order(state, settings, order, allow_partial=None):
    allow_partial = order.allow_backorder if allow_partial is None else allow_partial
    added = []
    for sku, line in sorted(order.lines.items()):
        needed = line.outstanding - reserved_for_order(state, order.tenant, order.order_id, sku)
        if needed <= 0:
            continue
        for warehouse, count in allocation_plan(state, settings, order.tenant, sku, needed, allow_partial):
            row = Reservation(order.tenant, state.next_id(order.tenant, "reservation"),
                              order.order_id, sku, warehouse, count)
            state.reservations[(order.tenant, row.reservation_id)] = row
            post_inventory(state, order.tenant, "reserve", warehouse, sku, 0, count,
                           order.order_id, row.reservation_id)
            added.append(row)
    if added:
        order.touch()
    return added
