SUBSCRIPTIONS = {
    "order.placed": ("receipts", "analytics"),
    "order.cancelled": ("receipts", "analytics"),
    "shipment.created": ("tracking", "analytics"),
    "return.accepted": ("receipts", "analytics"),
    "payment.captured": ("receipts", "accounting"),
    "credit.issued": ("receipts", "accounting"),
}


def destinations(event_type):
    return SUBSCRIPTIONS.get(event_type, ("analytics",))


def delivery_body(message, destination):
    data = dict(message["data"])
    if destination == "analytics":
        data.pop("payment_reference", None)
    return {"message_id": message["message_id"], "type": message["type"], "data": data}
