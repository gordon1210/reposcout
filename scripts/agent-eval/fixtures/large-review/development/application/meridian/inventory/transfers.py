from meridian.core.errors import require
from meridian.core.quantities import bounded_quantity
from meridian.inventory.repository import stock_bin
from meridian.inventory.availability import available_in_bin
from meridian.inventory.ledger import post_inventory


def transfer_stock(state, settings, tenant, sku, source, destination, units):
    require(source != destination, "same_warehouse", "A transfer needs distinct warehouses")
    origin = stock_bin(state, settings, tenant, source, sku)
    target = stock_bin(state, settings, tenant, destination, sku)
    units = bounded_quantity(units, available_in_bin(state, settings, tenant, source, sku))
    transfer_id = state.next_id(tenant, "transfer")
    origin.on_hand -= units
    target.on_hand += units
    post_inventory(state, tenant, "transfer-out", source, sku, -units, 0, reference=transfer_id)
    post_inventory(state, tenant, "transfer-in", destination, sku, units, 0, reference=transfer_id)
    return {"transfer_id": transfer_id, "sku": sku, "quantity": units,
            "source": source, "destination": destination}
