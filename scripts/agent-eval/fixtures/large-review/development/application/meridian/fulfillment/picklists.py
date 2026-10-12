from meridian.inventory.repository import reservation_rows
from meridian.orders.repository import get_order


def warehouse_picklist(state, tenant, warehouse):
    grouped = {}
    for reservation in reservation_rows(state, tenant):
        if reservation.warehouse != warehouse:
            continue
        order = get_order(state, tenant, reservation.order_id)
        group = grouped.setdefault(order.order_id, {"order_id": order.order_id,
                                  "customer_id": order.customer_id, "items": {}})
        group["items"][reservation.sku] = group["items"].get(reservation.sku, 0) + reservation.reserved
    return [{"order_id": row["order_id"], "customer_id": row["customer_id"],
             "items": [{"sku": sku, "quantity": count} for sku, count in sorted(row["items"].items())]}
            for _, row in sorted(grouped.items())]
