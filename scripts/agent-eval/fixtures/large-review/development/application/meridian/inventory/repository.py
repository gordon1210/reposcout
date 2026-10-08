from meridian.inventory.model import StockBin
from meridian.core.errors import require
from meridian.catalog.repository import get_product


def stock_bin(state, settings, tenant, warehouse, sku):
    require(warehouse in settings.warehouses[tenant], "warehouse_not_found", "Unknown warehouse", 404)
    product = get_product(state, tenant, sku, include_inactive=True)
    require(not product.components, "bundle_stock", "Stock is held for component SKUs")
    key = (tenant, warehouse, sku)
    if key not in state.stock:
        state.stock[key] = StockBin(tenant, warehouse, sku)
    return state.stock[key]


def reservation_rows(state, tenant, order_id=None, sku=None, include_closed=False):
    return sorted((row for (owner, _), row in state.reservations.items()
                   if owner == tenant and (order_id is None or row.order_id == order_id)
                   and (sku is None or row.sku == sku) and (include_closed or row.reserved > 0)),
                  key=lambda row: row.reservation_id)
