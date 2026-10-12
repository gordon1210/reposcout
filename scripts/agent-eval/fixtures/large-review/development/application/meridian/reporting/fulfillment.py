from collections import Counter
from meridian.orders.repository import list_orders
from meridian.orders.queries import backorders
from meridian.inventory.availability import reserved_for_order
from meridian.api.presenters import order_view


def fulfillment_report(state, tenant):
    statuses = Counter(order_view(state, order)["status"] for order in list_orders(state, tenant))
    outstanding = backorders(state, tenant, lambda order_id, sku: reserved_for_order(state, tenant, order_id, sku))
    shipments = [row for (owner, _), row in state.shipments.items() if owner == tenant]
    return {"status_counts": dict(sorted(statuses.items())), "backorders": outstanding,
            "shipment_count": len(shipments), "delivered_count": sum(row["delivered"] for row in shipments)}
