"""One cycle, one real order. Retry state persists while failed checkout effects roll back."""
from meridian.api.auth import Context
from meridian.api.presenters import order_view
from meridian.core.audit import record
from meridian.core.errors import DomainError, require
from meridian.core.transaction import atomic
from meridian.customers.accounts import get_customer
from meridian.orders.creation import create_order
from meridian.orders.repository import get_order
from meridian.pricing.quote import quote
from meridian.recurring.plans import text
from meridian.recurring.repository import cycle, subscription, require_active, check_version
from meridian.recurring.schedules import day, after, integer


RETRYABLE = frozenset({"insufficient_stock"})


def customer_context(current):
    # The administrative caller can execute consent; cannot select the billed customer.
    return Context(current.tenant, current.customer_id, frozenset({"customer"}))


def preview(app, context, cycle_id, body):
    current = cycle(app.state, context.tenant, cycle_id)
    require(current.status not in ("skipped", "void"), "recurring_cycle_inactive",
            "Skipped and void cycles cannot be checked out", 409)
    if current.order_id is not None:
        return {"cycle_id": cycle_id, "existing_order": order_view(app.state, get_order(
            app.state, context.tenant, current.order_id))}
    customer = get_customer(app.state, current.tenant, current.customer_id)
    priced = quote(app.state, app.settings, current.tenant, customer, current.items, current.checkout)
    return {"cycle_id": cycle_id, "due_date": current.due_date, "quote": priced,
            "pricing": "current_catalog_at_renewal", "reserves_stock": False}


def execute(app, context, cycle_id, body):
    current = cycle(app.state, context.tenant, cycle_id)
    as_of = day(body.get("as_of"), "as_of").isoformat()
    if current.status == "ordered":
        return {"cycle": execution_view(current), "order": order_view(app.state, get_order(
            app.state, current.tenant, current.order_id)), "replayed": True}
    owner = subscription(app.state, context.tenant, current.subscription_id)
    require_active(owner)
    require(current.status in ("planned", "retry_wait"), "recurring_cycle_inactive",
            "Only planned cycles or eligible retries can execute", 409)
    require(as_of >= current.due_date, "recurring_not_due", "Cycle date has not arrived", 409)
    require(current.next_attempt_date is None or as_of >= current.next_attempt_date,
            "recurring_retry_early", "Retry window has not arrived", 409)
    require(len(current.attempts) < current.maximum_attempts,
            "recurring_attempt_limit", "Cycle exhausted its attempt allowance", 409)
    attempt = len(current.attempts) + 1
    try:
        with atomic(app.state):
            order = create_order(app, customer_context(current),
                                 dict(current.checkout, items=current.items))
    except DomainError as error:
        # atomic replaces state dictionaries; the previous object is now detached.
        current = cycle(app.state, context.tenant, cycle_id)
        retry = error.code in RETRYABLE and attempt < current.maximum_attempts
        current.attempts.append({"attempt": attempt, "as_of": as_of, "outcome": "failed",
                                 "error_code": error.code})
        current.status = "retry_wait" if retry else "held"
        current.next_attempt_date = after(as_of, current.retry_days) if retry else None
        current.hold_reason = None if retry else error.code
        current.version += 1
        record(app.state, context, "recurring.renewal.failed", cycle_id,
               {"attempt": attempt, "error_code": error.code, "status": current.status})
        return {"cycle": execution_view(current), "order": None, "replayed": False}
    current.order_id = order.order_id
    current.status = "ordered"
    current.next_attempt_date = None
    current.hold_reason = None
    current.attempts.append({"attempt": attempt, "as_of": as_of, "outcome": "ordered",
                             "order_id": order.order_id})
    current.version += 1
    record(app.state, context, "recurring.renewal.ordered", cycle_id,
           {"subscription_id": current.subscription_id, "order_id": order.order_id, "attempt": attempt})
    return {"cycle": execution_view(current), "order": order_view(app.state, order), "replayed": False}


def hold(state, context, cycle_id, body):
    current = cycle(state, context.tenant, cycle_id)
    check_version(current, body)
    require(current.status in ("planned", "retry_wait"), "recurring_cycle_inactive",
            "Only unexecuted cycles can be held", 409)
    current.hold_reason = text(body.get("reason"), "reason", 240)
    current.status = "held"
    current.next_attempt_date = None
    current.version += 1
    record(state, context, "recurring.cycle.hold", cycle_id, {"reason": current.hold_reason})
    return current


def release(state, context, cycle_id, body):
    current = cycle(state, context.tenant, cycle_id)
    check_version(current, body)
    require_active(subscription(state, context.tenant, current.subscription_id))
    require(current.status == "held", "recurring_not_held", "Cycle is not held", 409)
    as_of = day(body.get("as_of"), "as_of").isoformat()
    require(as_of >= current.due_date, "recurring_not_due", "Cycle date has not arrived", 409)
    if current.attempts:
        require(as_of >= current.attempts[-1]["as_of"], "recurring_time_reversal",
                "Release cannot precede the previous attempt", 409)
    allowance = integer(body.get("additional_attempts", 0), "additional_attempts", 0, 3)
    maximum = current.maximum_attempts + allowance
    require(maximum <= 10 and maximum > len(current.attempts),
            "recurring_attempt_limit", "Release needs remaining attempts, at most ten in total", 409)
    current.maximum_attempts = maximum
    current.hold_reason = None
    current.status = "retry_wait" if current.attempts else "planned"
    current.next_attempt_date = as_of
    current.version += 1
    record(state, context, "recurring.cycle.release", cycle_id,
           {"as_of": as_of, "additional_attempts": allowance})
    return current


def execution_view(current):
    return {"cycle_id": current.cycle_id, "subscription_id": current.subscription_id,
            "index": current.index, "due_date": current.due_date, "status": current.status,
            "version": current.version, "order_id": current.order_id,
            "attempts": [dict(attempt) for attempt in current.attempts],
            "next_attempt_date": current.next_attempt_date, "hold_reason": current.hold_reason,
            "maximum_attempts": current.maximum_attempts, "skip_reason": current.skip_reason}
