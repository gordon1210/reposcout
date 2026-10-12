def fulfillment_status(order, reserved):
    ordered = sum(line.ordered for line in order.lines.values())
    shipped = sum(line.shipped for line in order.lines.values())
    cancelled = sum(line.cancelled for line in order.lines.values())
    returned = sum(line.returned for line in order.lines.values())
    if cancelled == ordered:
        return "cancelled"
    if shipped + cancelled == ordered:
        if returned == shipped and shipped > 0:
            return "returned"
        if returned > 0:
            return "partially_returned"
        return "fulfilled"
    if shipped > 0:
        return "partially_shipped"
    if reserved > 0:
        return "reserved"
    return "awaiting_stock"
