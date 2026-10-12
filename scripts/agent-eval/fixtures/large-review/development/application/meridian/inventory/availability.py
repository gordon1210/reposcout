from meridian.inventory.repository import reservation_rows, stock_bin


def held_in_bin(state, tenant, warehouse, sku):
    return sum(row.reserved for row in reservation_rows(state, tenant, sku=sku)
               if row.warehouse == warehouse)


def available_in_bin(state, settings, tenant, warehouse, sku):
    stock = stock_bin(state, settings, tenant, warehouse, sku)
    return stock.on_hand - held_in_bin(state, tenant, warehouse, sku)


def reserved_for_order(state, tenant, order_id, sku=None):
    return sum(row.reserved for row in reservation_rows(state, tenant, order_id, sku))


def stock_view(state, settings, tenant, sku):
    bins = []
    for warehouse in settings.warehouses[tenant]:
        stock = stock_bin(state, settings, tenant, warehouse, sku)
        held = held_in_bin(state, tenant, warehouse, sku)
        bins.append({"warehouse": warehouse, "on_hand": stock.on_hand,
                     "reserved": held, "available": stock.on_hand - held})
    return {"sku": sku, "on_hand": sum(row["on_hand"] for row in bins),
            "reserved": sum(row["reserved"] for row in bins),
            "available": sum(row["available"] for row in bins), "warehouses": bins}
