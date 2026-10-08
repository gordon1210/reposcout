from meridian.core.quantities import quantity
from meridian.core.errors import require
from meridian.inventory.repository import stock_bin
from meridian.inventory.availability import held_in_bin
from meridian.inventory.ledger import post_inventory


def receive_stock(state, settings, tenant, warehouse, sku, units, reference=None):
    units = quantity(units)
    stock = stock_bin(state, settings, tenant, warehouse, sku)
    stock.on_hand += units
    post_inventory(state, tenant, "receive", warehouse, sku, units, 0, reference=reference)
    return stock


def count_stock(state, settings, tenant, warehouse, sku, counted, reason):
    counted = quantity(counted, "counted", allow_zero=True)
    require(isinstance(reason, str) and reason.strip(), "reason_required", "A stock count needs a reason")
    stock = stock_bin(state, settings, tenant, warehouse, sku)
    require(counted >= held_in_bin(state, tenant, warehouse, sku),
            "reserved_stock", "A count cannot discard units reserved for orders", 409)
    delta = counted - stock.on_hand
    stock.on_hand = counted
    post_inventory(state, tenant, "count", warehouse, sku, delta, 0, reference=reason.strip())
    return stock
