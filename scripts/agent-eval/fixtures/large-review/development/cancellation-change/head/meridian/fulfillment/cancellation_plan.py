from meridian.core.errors import require


def cancellation_release_plan(reservations, requested_quantity):
    target = sum(reservation.reserved for reservation in reservations)
    remaining = target
    plan = []
    for reservation in reservations:
        selected = min(reservation.reserved, remaining)
        if selected > 0:
            plan.append((reservation, selected))
            remaining -= selected
        if remaining == 0:
            break
    require(remaining == 0, "reservation_changed", "Reservation changed during cancellation", 409)
    return plan
