from copy import deepcopy
from meridian.catalog.repository import get_product
from meridian.core.audit import record
from meridian.core.errors import require
from meridian.core.identity import identifier
from meridian.recurring.models import Plan
from meridian.recurring.repository import plan, check_version
from meridian.recurring.schedules import parse_schedule, integer


def text(value, field, maximum=160):
    require(isinstance(value, str) and 1 <= len(value.strip()) <= maximum,
            "invalid_recurring_text", f"{field} must contain 1..{maximum} characters")
    return value.strip()


def parse_items(state, tenant, items):
    require(isinstance(items, list) and 1 <= len(items) <= 30,
            "invalid_recurring_items", "A plan needs 1..30 product lines")
    merged = {}
    for item in items:
        require(isinstance(item, dict) and set(item) <= {"sku", "quantity"},
                "invalid_recurring_items", "Each plan item needs only sku and quantity")
        sku = identifier(item.get("sku"), "sku")
        product = get_product(state, tenant, sku)
        require(not product.components, "recurring_bundle", "Plans require individually orderable products")
        count = integer(item.get("quantity"), "quantity", 1, 1000)
        merged[sku] = integer(merged.get(sku, 0) + count, "combined quantity", 1, 1000)
    return [{"sku": sku, "quantity": quantity} for sku, quantity in sorted(merged.items())]


def create_plan(state, context, body):
    plan_id = identifier(body.get("plan_id"), "plan_id")
    require((context.tenant, plan_id) not in state.recurring_plans,
            "recurring_plan_exists", "Plan identifier already exists", 409)
    row = Plan(context.tenant, plan_id, text(body.get("name"), "name"),
               parse_items(state, context.tenant, body.get("items")),
               parse_schedule(body.get("schedule")),
               integer(body.get("maximum_multiplier", 10), "maximum_multiplier", 1, 100),
               integer(body.get("retry_days", 2), "retry_days", 1, 30),
               integer(body.get("maximum_attempts", 3), "maximum_attempts", 1, 10))
    state.recurring_plans[(context.tenant, plan_id)] = row
    record(state, context, "recurring.plan.create", plan_id, {"version": row.version})
    return row


def revise_plan(state, context, plan_id, body):
    row = plan(state, context.tenant, plan_id, active=True)
    check_version(row, body)
    require(set(body) <= {"version", "name", "items", "schedule", "maximum_multiplier",
                          "retry_days", "maximum_attempts"},
            "invalid_recurring_plan", "Unknown plan revision field")
    if "name" in body:
        row.name = text(body["name"], "name")
    if "items" in body:
        row.items = parse_items(state, context.tenant, body["items"])
    if "schedule" in body:
        row.schedule = parse_schedule(body["schedule"])
    for field, maximum in (("maximum_multiplier", 100), ("retry_days", 30), ("maximum_attempts", 10)):
        if field in body:
            setattr(row, field, integer(body[field], field, 1, maximum))
    row.version += 1
    record(state, context, "recurring.plan.revise", plan_id, {"version": row.version})
    return row


def retire_plan(state, context, plan_id, body):
    row = plan(state, context.tenant, plan_id)
    check_version(row, body)
    require(row.active, "recurring_plan_retired", "Plan is already retired", 409)
    row.active = False
    row.version += 1
    record(state, context, "recurring.plan.retire", plan_id)
    return row


def plan_view(row):
    return {"plan_id": row.plan_id, "name": row.name, "items": deepcopy(row.items),
            "schedule": dict(row.schedule), "maximum_multiplier": row.maximum_multiplier,
            "retry_days": row.retry_days, "maximum_attempts": row.maximum_attempts,
            "active": row.active, "version": row.version,
            "pricing": "current_catalog_at_renewal", "change_policy": "next_unplanned_cycle"}


def snapshot(row, multiplier):
    multiplier = integer(multiplier, "multiplier", 1, row.maximum_multiplier)
    items = [{"sku": item["sku"],
              "quantity": integer(item["quantity"] * multiplier, "multiplied quantity", 1, 10000)}
             for item in row.items]
    return {"plan_id": row.plan_id, "plan_version": row.version, "items": items,
            "schedule": dict(row.schedule), "multiplier": multiplier,
            "retry_days": row.retry_days, "maximum_attempts": row.maximum_attempts}
