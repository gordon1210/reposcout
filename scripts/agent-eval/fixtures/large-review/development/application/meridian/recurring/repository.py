from meridian.core.errors import require
from meridian.core.identity import identifier


def plan(state, tenant, plan_id, active=False):
    result = state.recurring_plans.get((tenant, identifier(plan_id, "plan_id")))
    require(result is not None, "recurring_plan_not_found", "Plan does not exist", 404)
    if active:
        require(result.active, "recurring_plan_retired", "Plan no longer accepts enrollments", 409)
    return result


def subscription(state, tenant, subscription_id, customer_id=None):
    result = state.recurring_subscriptions.get((tenant, identifier(subscription_id, "subscription_id")))
    require(result is not None and (customer_id is None or result.customer_id == customer_id),
            "subscription_not_found", "Subscription does not exist", 404)
    return result


def cycle(state, tenant, cycle_id, customer_id=None):
    result = state.recurring_cycles.get((tenant, identifier(cycle_id, "cycle_id")))
    require(result is not None and (customer_id is None or result.customer_id == customer_id),
            "recurring_cycle_not_found", "Cycle does not exist", 404)
    return result


def plans(state, tenant, active_only=True):
    return sorted((row for (owner, _), row in state.recurring_plans.items()
                   if owner == tenant and (row.active or not active_only)), key=lambda row: row.plan_id)


def subscriptions(state, tenant, customer_id=None):
    return sorted((row for (owner, _), row in state.recurring_subscriptions.items()
                   if owner == tenant and (customer_id is None or row.customer_id == customer_id)),
                  key=lambda row: row.subscription_id)


def cycles(state, tenant, subscription_id=None, customer_id=None):
    return sorted((row for (owner, _), row in state.recurring_cycles.items()
                   if owner == tenant and (subscription_id is None or row.subscription_id == subscription_id)
                   and (customer_id is None or row.customer_id == customer_id)),
                  key=lambda row: (row.due_date, row.subscription_id, row.index))


def check_version(row, body):
    value = body.get("version")
    require(type(value) is int and value > 0, "invalid_recurring_version", "A positive version is required")
    require(value == row.version, "recurring_version_conflict", "Resource changed; reload before editing", 409)


def require_active(row):
    require(row.status == "active", "subscription_stopped", "Subscription has been stopped", 409)
