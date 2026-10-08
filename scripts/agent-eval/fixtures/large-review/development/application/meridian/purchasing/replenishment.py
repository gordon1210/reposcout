"""Replenishment accounts for sellable stock, shortages, and already ordered units."""
from meridian.core.errors import require
from meridian.core.audit import record
from meridian.catalog.repository import get_product
from meridian.inventory.availability import available_in_bin, reserved_for_order
from meridian.orders.queries import backorders
from meridian.purchasing.orders import create_draft, remaining
from meridian.purchasing.validation import (integer, boolean, warehouse, lookup, snapshot, version)
from meridian.purchasing.suppliers import terms_for


def set_rule(state, settings, context, sku, body):
    product = get_product(state, context.tenant, sku)
    require(not product.components, "bundle_purchase", "Replenish component products")
    destination = warehouse(settings, context.tenant, body.get("warehouse"))
    terms = terms_for(state, context.tenant, body.get("supplier_id"), sku)
    minimum = integer(body.get("reorder_point"), "reorder_point", 0, 1_000_000)
    target = integer(body.get("target_quantity"), "target_quantity", 1, 1_000_000)
    require(target > minimum, "invalid_replenishment_target", "Target must exceed reorder point")
    key = (context.tenant, destination, sku)
    old = state.replenishment_rules.get(key)
    if old:
        version(old, body.get("expected_version"))
    else:
        require("expected_version" not in body, "stale_version", "Rule does not yet exist", 409)
    row = {"tenant": context.tenant, "warehouse": destination, "sku": sku,
           "supplier_id": terms["supplier_id"], "reorder_point": minimum, "target_quantity": target,
           "include_backorders": boolean(body.get("include_backorders", True), "include_backorders"),
           "active": boolean(body.get("active", True), "active"), "version": old["version"] + 1 if old else 1}
    state.replenishment_rules[key] = row
    record(state, context, "replenishment.rule_changed", sku, {"warehouse": destination})
    return snapshot(row)


def inventory_position(state, settings, tenant, destination, sku):
    available = available_in_bin(state, settings, tenant, destination, sku)
    on_order = 0
    quarantined = 0
    for (owner, _), purchase in state.purchase_orders.items():
        if owner != tenant or purchase["warehouse"] != destination or sku not in purchase["lines"]:
            continue
        if purchase["status"] in {"approved", "partially_received", "quarantined", "received"}:
            on_order += remaining(purchase["lines"][sku])
            quarantined += purchase["lines"][sku]["quarantined"]
    return {"available": available, "on_order": on_order, "quarantined": quarantined}


def propose(state, settings, tenant, destination=None):
    if destination is not None:
        warehouse(settings, tenant, destination)
    shortage_rows = backorders(state, tenant, lambda oid, sku: reserved_for_order(state, tenant, oid, sku))
    shortages = {}
    for order in shortage_rows:
        for sku, units in order["missing"].items():
            shortages[sku] = shortages.get(sku, 0) + units
    proposals = []
    warnings = []
    # Each SKU's global shortage is assigned once in configured warehouse order.
    # This keeps a network shortage from being counted independently by every bin.
    ordered_warehouses = {name: index for index, name in enumerate(settings.warehouses[tenant])}
    rules = sorted((row for (owner, wh, _), row in state.replenishment_rules.items()
                    if owner == tenant and row["active"]),
                   key=lambda row: (ordered_warehouses[row["warehouse"]], row["sku"]))
    consumed_shortages = set()
    for rule in rules:
        sku = rule["sku"]
        shortage = shortages.get(sku, 0) if rule["include_backorders"] and sku not in consumed_shortages else 0
        if rule["include_backorders"]:
            consumed_shortages.add(sku)
        if destination is not None and rule["warehouse"] != destination:
            continue
        position = inventory_position(state, settings, tenant, rule["warehouse"], sku)
        net = position["available"] + position["on_order"] - shortage
        if net > rule["reorder_point"]:
            continue
        supplier_id = rule["supplier_id"]
        supplier = state.purchase_suppliers.get((tenant, supplier_id))
        terms = state.purchase_terms.get((tenant, supplier_id, sku))
        product = state.products.get((tenant, sku))
        if not supplier or supplier["status"] != "active" or not terms or not terms["active"] or not product or not product.active:
            warnings.append({"sku": sku, "warehouse": rule["warehouse"], "kind": "supplier_terms_unavailable"})
            continue
        required = max(rule["target_quantity"] - net, terms["minimum_quantity"])
        units = ((required + terms["pack_size"] - 1) // terms["pack_size"]) * terms["pack_size"]
        if units > 1_000_000:
            warnings.append({"sku": sku, "warehouse": rule["warehouse"], "kind": "quantity_limit"})
            continue
        proposals.append({"sku": sku, "warehouse": rule["warehouse"], "supplier_id": supplier_id,
                          "quantity": units, "unit_cents": terms["unit_cents"], "cost_cents": units * terms["unit_cents"],
                          "lead_days": terms["lead_days"], "terms_version": terms["version"],
                          "rule_version": rule["version"], "backorder_quantity": shortage,
                          "position": position, "net_position": net, "target_quantity": rule["target_quantity"]})
    return {"proposals": proposals, "warnings": warnings,
            "total_cents": sum(row["cost_cents"] for row in proposals)}


def save_plan(state, settings, context, body):
    destination = body.get("warehouse")
    proposal = propose(state, settings, context.tenant, destination)
    require(proposal["proposals"], "replenishment_empty", "No replenishment is needed", 409)
    plan_id = state.next_id(context.tenant, "replenishment")
    row = {"plan_id": plan_id, "tenant": context.tenant, "warehouse": destination,
           "status": "proposed", "version": 1, "evidence": proposal, "purchase_order_ids": []}
    state.replenishment_plans[(context.tenant, plan_id)] = row
    record(state, context, "replenishment.proposed", plan_id)
    return snapshot(row)


def convert_plan(state, settings, context, plan_id, body):
    plan = lookup(state.replenishment_plans, context.tenant, plan_id, "replenishment_plan")
    if plan["status"] == "converted":
        return snapshot(plan)
    version(plan, body.get("expected_version"))
    current = propose(state, settings, context.tenant, plan["warehouse"])
    require(current == plan["evidence"], "replenishment_stale", "Inventory, demand, rules, or terms changed; create a new plan", 409)
    # Drafts do not count as inbound inventory, but must not be duplicated by a
    # second saved plan generated from exactly the same observation.
    for (tenant, other_id), other in state.replenishment_plans.items():
        if tenant != context.tenant or other_id == plan_id or other["status"] != "converted":
            continue
        for order_id in other["purchase_order_ids"]:
            purchase = state.purchase_orders[(tenant, order_id)]
            if purchase["status"] == "draft":
                occupied = {(purchase["warehouse"], sku) for sku in purchase["lines"]}
                require(not any((row["warehouse"], row["sku"]) in occupied for row in current["proposals"]),
                        "replenishment_draft_exists", "An unapproved replenishment draft already covers a proposed SKU", 409)
    groups = {}
    for item in current["proposals"]:
        groups.setdefault((item["supplier_id"], item["warehouse"]), []).append({"sku": item["sku"], "quantity": item["quantity"]})
    for (vendor, destination), lines in sorted(groups.items()):
        draft = create_draft(state, settings, context, {"supplier_id": vendor, "warehouse": destination, "lines": lines})
        state.purchase_orders[(context.tenant, draft["purchase_order_id"])]["source_plan_id"] = plan_id
        plan["purchase_order_ids"].append(draft["purchase_order_id"])
    plan.update(status="converted", version=plan["version"] + 1)
    record(state, context, "replenishment.converted", plan_id, {"purchase_order_ids": list(plan["purchase_order_ids"])})
    return snapshot(plan)
