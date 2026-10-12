from meridian.catalog.repository import list_products
from meridian.inventory.availability import stock_view
from meridian.inventory.reconciliation import reconcile_inventory


def stock_report(state, settings, tenant):
    rows = [stock_view(state, settings, tenant, product.sku)
            for product in sorted(list_products(state, tenant), key=lambda row: row.sku)
            if not product.components]
    return {"stock": rows, "total_on_hand": sum(row["on_hand"] for row in rows),
            "total_reserved": sum(row["reserved"] for row in rows),
            "reconciliation": reconcile_inventory(state, tenant)}
