"""Customer intent changes future unplanned cycles, never already-created orders."""
from meridian.core.audit import record
from meridian.core.errors import require
from meridian.customers.accounts import get_customer
from meridian.pricing.quote import quote
from meridian.recurring.enrollment import checkout_options
from meridian.recurring.plans import snapshot, text
from meridian.recurring.repository import subscription, plan, cycles, check_version, require_active
from meridian.recurring.schedules import window, overlaps, occurrence, day, integer


def mutable_subscription(state, context, subscription_id, body):
    row = subscription(state, context.tenant, subscription_id, context.actor)
    check_version(row, body)
    require_active(row)
    return row


def pause(state, context, subscription_id, body):
    row = mutable_subscription(state, context, subscription_id, body)
    value = window(body.get("pause"))
    next_date = occurrence(row.start_date, row.schedule, row.next_index)
    require(value["through"] >= next_date, "pause_in_past", "Pause must cover future unplanned dates")
    require(not any(overlaps(value, existing) for existing in row.pauses),
            "pause_overlap", "Pause overlaps an existing pause", 409)
    require(len(row.pauses) < 24, "pause_limit", "At most 24 pause windows are supported")
    row.pauses.append(value)
    row.pauses.sort(key=lambda item: item["from"])
    row.version += 1
    record(state, context, "recurring.pause", row.subscription_id, value)
    return row


def remove_pause(state, context, subscription_id, body):
    row = mutable_subscription(state, context, subscription_id, body)
    value = window(body.get("pause"))
    require(value in row.pauses, "pause_not_found", "Pause window does not exist", 404)
    row.pauses.remove(value)
    row.version += 1
    record(state, context, "recurring.resume", row.subscription_id, value)
    return row


def skip(state, context, subscription_id, body):
    row = mutable_subscription(state, context, subscription_id, body)
    index = integer(body.get("index"), "index", row.next_index, row.next_index + 100)
    # The index is stable across month-end clamping and cannot name an old order.
    due = occurrence(row.start_date, row.schedule, index)
    reason = text(body.get("reason", "customer_requested"), "reason", 240)
    require(index not in row.skips, "cycle_already_skipped", "Cycle already has a skip request", 409)
    require(len(row.skips) < 100, "skip_limit", "At most 100 outstanding skips are supported")
    row.skips[index] = reason
    row.version += 1
    record(state, context, "recurring.skip", row.subscription_id, {"index": index, "due_date": due})
    return row


def unskip(state, context, subscription_id, body):
    row = mutable_subscription(state, context, subscription_id, body)
    index = integer(body.get("index"), "index", row.next_index, row.next_index + 100)
    require(index in row.skips, "skip_not_found", "Cycle has no outstanding skip", 404)
    del row.skips[index]
    row.version += 1
    record(state, context, "recurring.unskip", row.subscription_id, {"index": index})
    return row


def change_plan(app, context, subscription_id, body):
    row = mutable_subscription(app.state, context, subscription_id, body)
    target = plan(app.state, context.tenant, body.get("plan_id"), active=True)
    selected = snapshot(target, body.get("multiplier", row.multiplier))
    # Cadence changes would rename future dates and invalidate customer skip consent.
    require(selected["schedule"] == row.schedule, "recurring_cadence_change",
            "A different cadence requires a new enrollment")
    customer = get_customer(app.state, context.tenant, context.actor)
    quote(app.state, app.settings, context.tenant, customer, selected["items"], row.checkout)
    row.pending_change = selected
    row.version += 1
    record(app.state, context, "recurring.plan.schedule", row.subscription_id,
           {"plan_id": target.plan_id, "effective_index": row.next_index})
    return row


def discard_change(state, context, subscription_id, body):
    row = mutable_subscription(state, context, subscription_id, body)
    require(row.pending_change is not None, "recurring_change_not_found", "No plan change is pending", 404)
    row.pending_change = None
    row.version += 1
    record(state, context, "recurring.plan.discard", row.subscription_id)
    return row


def change_checkout(app, context, subscription_id, body):
    row = mutable_subscription(app.state, context, subscription_id, body)
    options = checkout_options(body.get("checkout"))
    customer = get_customer(app.state, context.tenant, context.actor)
    quote(app.state, app.settings, context.tenant, customer, row.items, options)
    if row.pending_change:
        quote(app.state, app.settings, context.tenant, customer, row.pending_change["items"], options)
    row.checkout = options
    row.version += 1
    record(app.state, context, "recurring.checkout.change", row.subscription_id, options)
    return row


def stop(state, context, subscription_id, body):
    row = mutable_subscription(state, context, subscription_id, body)
    reason = text(body.get("reason", "customer_requested"), "reason", 240)
    row.status = "stopped"
    row.stop_reason = reason
    row.pending_change = None
    row.version += 1
    suppressed = []
    for current in cycles(state, context.tenant, row.subscription_id):
        if current.status in ("planned", "retry_wait", "held"):
            current.status = "void"
            current.hold_reason = None
            current.next_attempt_date = None
            current.skip_reason = "subscription_stopped"
            current.version += 1
            suppressed.append(current.cycle_id)
    record(state, context, "recurring.stop", row.subscription_id,
           {"reason": reason, "void_cycles": suppressed})
    return row
