from meridian.inventory.adjustments import receive_stock
from meridian.inventory.availability import stock_view
from meridian.billing.payments import capture_payment
from meridian.billing.credits import issue_credit
from meridian.fulfillment.tracking import record_tracking


def stock_received(app, envelope):
    data = envelope.data
    stock = receive_stock(app.state, app.settings, envelope.tenant, data.get("warehouse"),
                          data.get("sku"), data.get("quantity"), envelope.event_id)
    view = stock_view(app.state, app.settings, envelope.tenant, stock.sku)
    return {"sku": stock.sku, "on_hand": view["on_hand"], "available": view["available"]}


def payment_captured(app, envelope):
    data = envelope.data
    return capture_payment(app.state, envelope.tenant, data.get("order_id"),
                           data.get("amount_cents"), data.get("payment_reference"))


def credit_requested(app, envelope):
    data = envelope.data
    return issue_credit(app.state, envelope.tenant, data.get("order_id"),
                        data.get("amount_cents"), "provider-adjustment")


def shipment_tracked(app, envelope):
    data = envelope.data
    return record_tracking(app.state, envelope.tenant, data.get("shipment_id"),
                           data.get("status"), data.get("location"))
