from meridian.fulfillment.status import fulfillment_status
from meridian.inventory.repository import reservation_rows
from meridian.inventory.availability import reserved_for_order
from meridian.orders.totals import order_totals
from meridian.orders.notes import customer_notes
from meridian.billing.invoices import invoice_view


def order_view(state, order):
    reserved = reserved_for_order(state, order.tenant, order.order_id)
    status = fulfillment_status(order, reserved)
    lines = []
    for sku, line in sorted(order.lines.items()):
        held = reserved_for_order(state, order.tenant, order.order_id, sku)
        lines.append({"sku": sku, "ordered": line.ordered, "shipped": line.shipped,
                      "cancelled": line.cancelled, "returned": line.returned,
                      "reserved": held, "unallocated": line.outstanding - held})
    rows = reservation_rows(state, order.tenant, order.order_id, include_closed=True)
    return {"order_id": order.order_id, "version": order.version, "status": status,
            "lines": lines, "reservations": [row.public() for row in rows],
            "totals": order_totals(order), "invoice": invoice_view(state, order.tenant, order.order_id),
            "notes": customer_notes(order)}
