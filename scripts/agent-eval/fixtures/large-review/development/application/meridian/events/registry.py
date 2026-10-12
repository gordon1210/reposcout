from meridian.core.errors import require
from meridian.events.handlers import stock_received, payment_captured, credit_requested, shipment_tracked

EVENT_HANDLERS = {
    "stock.received": stock_received,
    "payment.captured": payment_captured,
    "credit.requested": credit_requested,
    "shipment.tracked": shipment_tracked,
}


def event_handler(event_type):
    handler = EVENT_HANDLERS.get(event_type)
    require(handler is not None, "unknown_event_type", "Provider event type is not registered")
    return handler
