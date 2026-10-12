"""Bounded, deterministic due-cycle materialization; this module never places orders."""
from copy import deepcopy
from meridian.core.audit import record
from meridian.core.errors import require
from meridian.recurring.models import Cycle
from meridian.recurring.repository import subscriptions, subscription, cycles
from meridian.recurring.schedules import day, integer, boolean, occurrence, contains


def cycle_spec(row, index):
    due = occurrence(row.start_date, row.schedule, index)
    reason = row.skips.get(index)
    if reason is None and any(contains(pause, due) for pause in row.pauses):
        reason = "pause_window"
    selected = row.pending_change or {"plan_id": row.plan_id, "plan_version": row.plan_version,
                                      "items": row.items, "retry_days": row.retry_days,
                                      "maximum_attempts": row.maximum_attempts}
    return {"subscription_id": row.subscription_id, "index": index, "due_date": due,
            "plan_id": selected["plan_id"], "plan_version": selected["plan_version"],
            "items": deepcopy(selected["items"]), "checkout": dict(row.checkout),
            "retry_days": selected["retry_days"], "maximum_attempts": selected["maximum_attempts"],
            "status": "skipped" if reason is not None else "planned", "skip_reason": reason}


def due_specs(state, tenant, through, limit, subscription_id=None):
    """Merge per-subscription next dates so request size cannot starve an earlier date."""
    candidates = ([subscription(state, tenant, subscription_id)] if subscription_id is not None
                  else subscriptions(state, tenant))
    positions = {row.subscription_id: row.next_index for row in candidates if row.status == "active"}
    by_id = {row.subscription_id: row for row in candidates}
    result = []
    while len(result) < limit:
        due = [(occurrence(by_id[key].start_date, by_id[key].schedule, index), key, index)
               for key, index in positions.items()]
        due = [item for item in due if item[0] <= through]
        if not due:
            break
        _, key, index = min(due)
        result.append(cycle_spec(by_id[key], index))
        positions[key] += 1
    has_more = any(occurrence(by_id[key].start_date, by_id[key].schedule, index) <= through
                   for key, index in positions.items())
    return result, has_more


def apply_change(row):
    if row.pending_change is None:
        return
    for field in ("plan_id", "plan_version", "items", "schedule", "multiplier", "retry_days", "maximum_attempts"):
        setattr(row, field, deepcopy(row.pending_change[field]))
    row.pending_change = None


def plan_due(state, context, body):
    through = day(body.get("through"), "through").isoformat()
    limit = integer(body.get("limit", 50), "limit", 1, 100)
    dry_run = boolean(body.get("dry_run", False), "dry_run")
    specs, has_more = due_specs(state, context.tenant, through, limit, body.get("subscription_id"))
    if dry_run:
        return {"cycles": specs, "has_more": has_more, "dry_run": True, "through": through}
    result = []
    for spec in specs:
        row = subscription(state, context.tenant, spec["subscription_id"])
        require(row.next_index == spec["index"], "recurring_plan_conflict", "Cycle position changed", 409)
        apply_change(row)
        cycle_id = state.next_id(context.tenant, "cycle")
        current = Cycle(context.tenant, cycle_id, row.subscription_id, row.customer_id,
                        spec["index"], spec["due_date"], spec["plan_id"], spec["plan_version"],
                        spec["items"], spec["checkout"], spec["retry_days"], spec["maximum_attempts"],
                        status=spec["status"], skip_reason=spec["skip_reason"])
        state.recurring_cycles[(context.tenant, cycle_id)] = current
        row.skips.pop(row.next_index, None)
        row.next_index += 1
        row.version += 1
        record(state, context, "recurring.cycle.plan", cycle_id,
               {"subscription_id": row.subscription_id, "index": current.index,
                "status": current.status, "due_date": current.due_date})
        result.append(dict(spec, cycle_id=cycle_id))
    return {"cycles": result, "has_more": has_more, "dry_run": False, "through": through}


def forecast(state, context, subscription_id, body):
    row = subscription(state, context.tenant, subscription_id, context.actor)
    count = integer(body.get("count", 6), "count", 1, 24)
    future = [] if row.status != "active" else [cycle_spec(row, row.next_index + offset) for offset in range(count)]
    materialized = [{"cycle_id": current.cycle_id, "index": current.index,
                     "due_date": current.due_date, "status": current.status}
                    for current in cycles(state, context.tenant, subscription_id)
                    if current.status in ("planned", "retry_wait", "held")]
    return {"subscription_id": subscription_id, "version": row.version,
            "planned_cycles": materialized, "future_cycles": future}
