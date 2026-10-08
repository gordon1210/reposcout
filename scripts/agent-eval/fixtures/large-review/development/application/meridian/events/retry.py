from meridian.core.errors import require


def retry_at(attempt, tick):
    require(type(tick) is int and tick >= 0, "invalid_tick", "Tick must be a nonnegative integer")
    return tick + min(2 ** max(attempt - 1, 0), 64)


def ready_for_delivery(delivery, tick):
    return not delivery["delivered"] and delivery["attempts"] < 5 and delivery["next_tick"] <= tick


def failed_delivery(delivery, tick, reason):
    delivery["attempts"] += 1
    delivery["next_tick"] = retry_at(delivery["attempts"], tick)
    delivery["last_error"] = reason
