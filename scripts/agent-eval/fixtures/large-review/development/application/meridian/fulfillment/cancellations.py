from meridian.orders.repository import get_line
from meridian.orders.totals import line_value
from meridian.fulfillment.cancellation_policy import cancellable_quantity
from meridian.fulfillment.cancellation_plan import cancellation_release_plan
from meridian.inventory.repository import reservation_rows
from meridian.inventory.ledger import post_inventory
from meridian.billing.credits import issue_credit
from meridian.events.outbox import publish
from meridian.core.audit import record


def cancel_reserved_units(state, context, order, sku, requested):
    line = get_line(order, sku)
    count = cancellable_quantity(state, order, line, requested)
    if count == 0:
        return {"cancelled_quantity": 0, "credit_cents": 0}
    rows = reservation_rows(state, context.tenant, order.order_id, sku)
    plan = cancellation_release_plan(rows, count)
    before_credit = line_value(line, line.cancelled)
    for reservation, released in plan:
        reservation.reserved -= released
        reservation.released += released
        post_inventory(state, context.tenant, "release", reservation.warehouse, sku, 0,
                       -released, order.order_id, reservation.reservation_id)
    line.cancelled += count
    credit_cents = line_value(line, line.cancelled) - before_credit
    issue_credit(state, context.tenant, order.order_id, credit_cents, "cancellation")
    order.touch()
    publish(state, context.tenant, "order.cancelled", {"order_id": order.order_id, "sku": sku, "quantity": count})
    record(state, context, "order.cancel", order.order_id, {"sku": sku, "quantity": count})
    return {"cancelled_quantity": count, "credit_cents": credit_cents}
