from meridian.inventory.availability import held_in_bin, reserved_for_order
from meridian.inventory.ledger import inventory_entries
from meridian.orders.repository import list_orders


def reconcile_inventory(state, tenant):
    issues = []
    for (owner, warehouse, sku), stock in sorted(state.stock.items()):
        if owner != tenant:
            continue
        entries = [entry for entry in inventory_entries(state, tenant, sku=sku)
                   if entry["warehouse"] == warehouse]
        held = held_in_bin(state, tenant, warehouse, sku)
        ledger_on_hand = sum(entry["on_hand_delta"] for entry in entries)
        ledger_reserved = sum(entry["reserved_delta"] for entry in entries)
        if stock.on_hand != ledger_on_hand or held != ledger_reserved or held > stock.on_hand:
            issues.append({"kind": "bin_ledger", "warehouse": warehouse, "sku": sku,
                           "on_hand": stock.on_hand, "ledger_on_hand": ledger_on_hand,
                           "reserved": held, "ledger_reserved": ledger_reserved})
    for order in list_orders(state, tenant):
        for sku, line in sorted(order.lines.items()):
            held = reserved_for_order(state, tenant, order.order_id, sku)
            if held > line.outstanding or (not order.allow_backorder and held != line.outstanding):
                issues.append({"kind": "order_reservation", "order_id": order.order_id,
                               "sku": sku, "outstanding": line.outstanding, "reserved": held})
    return {"consistent": not issues, "issues": issues}
