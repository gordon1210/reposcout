from meridian.core.errors import require
from meridian.events.subscriptions import destinations, delivery_body
from meridian.events.retry import ready_for_delivery, failed_delivery


def publish(state, tenant, event_type, data):
    message_id = state.next_id(tenant, "message")
    deliveries = {destination: {"delivered": False, "attempts": 0, "next_tick": 0, "last_error": None}
                  for destination in destinations(event_type)}
    message = {"message_id": message_id, "tenant": tenant, "type": event_type,
               "data": dict(data), "deliveries": deliveries}
    state.outbox[(tenant, message_id)] = message
    return message_id


def drain_outbox(state, tenant, tick, simulate_failure=False):
    require(type(tick) is int and tick >= 0, "invalid_tick", "Tick must be nonnegative")
    result = []
    for (owner, _), message in sorted(state.outbox.items()):
        if owner != tenant:
            continue
        for destination, delivery in message["deliveries"].items():
            if not ready_for_delivery(delivery, tick):
                continue
            if simulate_failure:
                failed_delivery(delivery, tick, "local delivery rejected")
            else:
                delivery["attempts"] += 1
                delivery["delivered"] = True
                delivery["last_error"] = None
                result.append({"destination": destination, "body": delivery_body(message, destination)})
    return {"deliveries": result, "delivered_count": len(result)}


def outbox_summary(state, tenant):
    deliveries = [delivery for (owner, _), message in state.outbox.items() if owner == tenant
                  for delivery in message["deliveries"].values()]
    return {"pending": sum(not row["delivered"] and row["attempts"] < 5 for row in deliveries),
            "delivered": sum(row["delivered"] for row in deliveries),
            "exhausted": sum(not row["delivered"] and row["attempts"] >= 5 for row in deliveries)}
