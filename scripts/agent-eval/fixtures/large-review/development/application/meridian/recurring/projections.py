"""Standing-order entitlement accounting uses actual units and invoice balances."""
from copy import deepcopy
from meridian.api.presenters import order_view
from meridian.billing.invoices import invoice_view
from meridian.core.errors import require
from meridian.orders.repository import get_order
from meridian.recurring.repository import cycles, subscriptions
from meridian.recurring.renewals import execution_view
from meridian.recurring.schedules import occurrence, integer


def subscription_view(row):
    return {"subscription_id": row.subscription_id, "plan_id": row.plan_id,
            "plan_version": row.plan_version, "items": deepcopy(row.items),
            "schedule": dict(row.schedule), "start_date": row.start_date,
            "multiplier": row.multiplier, "checkout": dict(row.checkout),
            "status": row.status, "version": row.version, "next_index": row.next_index,
            "next_date": occurrence(row.start_date, row.schedule, row.next_index)
            if row.status == "active" else None,
            "pauses": deepcopy(row.pauses),
            "skips": [{"index": index, "reason": reason} for index, reason in sorted(row.skips.items())],
            "pending_change": deepcopy(row.pending_change), "stop_reason": row.stop_reason,
            "retry_days": row.retry_days, "maximum_attempts": row.maximum_attempts}


def cycle_view(state, current):
    result = dict(execution_view(current), plan_id=current.plan_id, plan_version=current.plan_version,
                  items=deepcopy(current.items), checkout=dict(current.checkout))
    result["order"] = order_view(state, get_order(state, current.tenant, current.order_id)) if current.order_id else None
    return result


def unit_account(state, tenant, rows):
    """No prepaid period exists: skipped deliveries consume no stock and incur no invoice."""
    units = {}
    invoiced = captured = credited = due = 0
    statuses = {}
    for current in rows:
        statuses[current.status] = statuses.get(current.status, 0) + 1
        for item in current.items:
            row = units.setdefault(item["sku"], {"sku": item["sku"], "scheduled": 0, "skipped": 0,
                                                "void": 0, "pending": 0, "ordered": 0,
                                                "shipped": 0, "cancelled": 0, "returned": 0})
            row["scheduled"] += item["quantity"]
            if current.status in ("skipped", "void"):
                row[current.status] += item["quantity"]
            elif current.status != "ordered":
                row["pending"] += item["quantity"]
        if current.order_id:
            order = get_order(state, tenant, current.order_id)
            invoice = invoice_view(state, tenant, current.order_id)
            invoiced += invoice["total_cents"]
            captured += invoice["captured_cents"]
            credited += invoice["credited_cents"]
            due += invoice["due_cents"]
            for sku, line in order.lines.items():
                row = units[sku]
                row["ordered"] += line.ordered
                row["shipped"] += line.shipped
                row["cancelled"] += line.cancelled
                row["returned"] += line.returned
    return {"units": [units[key] for key in sorted(units)], "cycle_counts": statuses,
            "billing": {"currency": "EUR", "invoiced_cents": invoiced, "captured_cents": captured,
                        "credited_cents": credited, "due_cents": due},
            "billing_policy": "Only placed orders are invoiced; skips and pauses have no charge."}


def statement(state, context, row):
    rows = cycles(state, context.tenant, row.subscription_id, context.actor)
    return dict(unit_account(state, context.tenant, rows), subscription=subscription_view(row),
                cycles=[execution_view(current) for current in rows])


def customer_listing(state, context, body):
    limit = integer(body.get("limit", 25), "limit", 1, 100)
    status = body.get("status")
    require(status in (None, "active", "stopped"), "invalid_recurring_status", "Unknown subscription status")
    cursor = body.get("after")
    require(cursor is None or isinstance(cursor, str), "invalid_recurring_cursor", "after must be text")
    rows = [row for row in subscriptions(state, context.tenant, context.actor)
            if (status is None or row.status == status) and (cursor is None or row.subscription_id > cursor)]
    selected = rows[:limit]
    return {"subscriptions": [subscription_view(row) for row in selected],
            "next_cursor": selected[-1].subscription_id if len(rows) > limit else None}


def operational_report(state, context, body):
    status = body.get("status")
    require(status in (None, "planned", "retry_wait", "held", "ordered", "skipped", "void"),
            "invalid_recurring_status", "Unknown cycle status")
    limit = integer(body.get("limit", 50), "limit", 1, 100)
    all_rows = cycles(state, context.tenant, body.get("subscription_id"))
    selected = [row for row in all_rows if status is None or row.status == status]
    rows = selected[:limit]
    return {"cycles": [execution_view(row) for row in rows], "total": len(selected),
            "truncated": len(selected) > limit, "accounting": unit_account(state, context.tenant, all_rows)}


def audit_trail(state, context, row):
    cycle_ids = {current.cycle_id for current in cycles(state, context.tenant, row.subscription_id)}
    subjects = cycle_ids | {row.subscription_id}
    return {"subscription_id": row.subscription_id,
            "entries": [deepcopy(entry) for entry in state.audit
                        if entry["tenant"] == context.tenant and entry["subject"] in subjects
                        and entry["action"].startswith("recurring.")]}
