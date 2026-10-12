from copy import deepcopy
from meridian.core.audit import record
from meridian.core.errors import require
from meridian.core.identity import identifier
from meridian.customers.accounts import get_customer
from meridian.pricing.quote import quote
from meridian.recurring.models import Subscription
from meridian.recurring.repository import plan, subscriptions
from meridian.recurring.plans import snapshot
from meridian.recurring.schedules import day, boolean


def checkout_options(value):
    require(isinstance(value, dict), "invalid_recurring_checkout", "checkout must be an object")
    require(set(value) <= {"allow_backorder", "express", "pickup", "region"},
            "invalid_recurring_checkout", "Checkout permits shipping choices and backorder consent only")
    options = {field: boolean(value.get(field, False), field)
               for field in ("allow_backorder", "express", "pickup")}
    require(not (options["express"] and options["pickup"]),
            "invalid_recurring_checkout", "Pickup cannot request express delivery")
    if "region" in value:
        require(value["region"] in ("domestic", "international"),
                "invalid_recurring_region", "Unknown shipping region")
        options["region"] = value["region"]
    return options


def enroll(app, context, body):
    customer = get_customer(app.state, context.tenant, context.actor)
    key = identifier(body.get("enrollment_key"), "enrollment_key")
    plan_id = identifier(body.get("plan_id"), "plan_id")
    start_date = day(body.get("start_date"), "start_date").isoformat()
    options = checkout_options(body.get("checkout", {}))
    # Compare the original request, not today's catalog revision, for safe retries.
    multiplier = body.get("multiplier", 1)
    require(type(multiplier) is int and multiplier > 0,
            "invalid_recurring_number", "multiplier must be a positive integer")
    fingerprint = {"plan_id": plan_id, "start_date": start_date,
                   "multiplier": multiplier, "checkout": options}
    for existing in subscriptions(app.state, context.tenant, context.actor):
        if existing.enrollment_key == key:
            require(existing.enrollment_fingerprint == fingerprint,
                    "recurring_enrollment_conflict", "Enrollment key was used with different choices", 409)
            return existing
    selected = snapshot(plan(app.state, context.tenant, plan_id, active=True), multiplier)
    quote(app.state, app.settings, context.tenant, customer, selected["items"], options)
    subscription_id = app.state.next_id(context.tenant, "subscription")
    row = Subscription(context.tenant, subscription_id, context.actor, key,
                       deepcopy(fingerprint), selected["plan_id"], selected["plan_version"],
                       selected["items"], selected["schedule"], start_date, multiplier,
                       options, selected["retry_days"], selected["maximum_attempts"])
    app.state.recurring_subscriptions[(context.tenant, subscription_id)] = row
    record(app.state, context, "recurring.enroll", subscription_id,
           {"plan_id": row.plan_id, "start_date": start_date})
    return row
