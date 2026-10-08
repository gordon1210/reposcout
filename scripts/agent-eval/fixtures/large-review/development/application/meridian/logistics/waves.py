"""Warehouse work queues claim reserved units without moving physical stock."""
from meridian.core.audit import record
from meridian.core.errors import require
from meridian.events.outbox import publish
from meridian.fulfillment.picklists import warehouse_picklist
from meridian.inventory.repository import reservation_rows
from meridian.logistics.common import integer, lookup, public, text, warehouse


def committed_units(state, tenant, warehouse_id, excluding=None):
    result = {}
    for (owner, wave_id), wave in state.logistics_waves.items():
        if owner != tenant or wave_id == excluding or wave["warehouse"] != warehouse_id:
            continue
        if wave["status"] in {"cancelled", "dispatched"}:
            continue
        for line in wave["lines"]:
            key = (line["order_id"], line["sku"])
            result[key] = result.get(key, 0) + line["quantity"]
    return result


def plan_wave(state, settings, tenant, body):
    warehouse_id = warehouse(settings, tenant, body.get("warehouse"))
    max_orders = integer(body.get("max_orders", 20), "max_orders", 1, 100)
    max_units = integer(body.get("max_units", 500), "max_units", 1, 10000)
    selected = body.get("order_ids")
    require(selected is None or (isinstance(selected, list) and 1 <= len(selected) <= 100
            and all(isinstance(value, str) for value in selected)),
            "invalid_wave_orders", "order_ids must be a bounded list of IDs")
    require(selected is None or len(selected) == len(set(selected)),
            "duplicate_wave_order", "An order can occur only once")
    claimed = committed_units(state, tenant, warehouse_id)
    orders = warehouse_picklist(state, tenant, warehouse_id)
    if selected is not None:
        visible = {row["order_id"] for row in orders}
        require(set(selected).issubset(visible), "wave_order_unavailable",
                "A requested order has no reserved units at this warehouse", 409)
        orders = [row for row in orders if row["order_id"] in selected]
    lines, order_ids, skipped = [], [], []
    remaining = max_units
    for order in orders:
        available = [{"order_id": order["order_id"], "sku": item["sku"],
                      "quantity": max(0, item["quantity"] - claimed.get((order["order_id"], item["sku"]), 0)),
                      "picked": 0}
                     for item in order["items"]]
        available = [row for row in available if row["quantity"]]
        total = sum(row["quantity"] for row in available)
        if not total:
            skipped.append({"order_id": order["order_id"], "reason": "already_claimed"})
        elif len(order_ids) >= max_orders or total > remaining:
            skipped.append({"order_id": order["order_id"], "reason": "capacity"})
        else:
            lines.extend(available)
            order_ids.append(order["order_id"])
            remaining -= total
    return {"warehouse": warehouse_id, "lines": lines, "order_ids": order_ids,
            "unit_count": max_units - remaining, "skipped": skipped}


def create_wave(state, settings, context, body):
    plan = plan_wave(state, settings, context.tenant, body)
    require(plan["lines"], "empty_wave", "No unclaimed reserved units fit this wave", 409)
    wave_id = state.next_id(context.tenant, "wave")
    row = dict(plan, wave_id=wave_id, tenant=context.tenant, status="planned", picker=None)
    state.logistics_waves[(context.tenant, wave_id)] = row
    record(state, context, "logistics.wave.create", wave_id, {"unit_count": row["unit_count"]})
    return public(row)


def validate_reservations(state, tenant, wave):
    available = {}
    for row in reservation_rows(state, tenant):
        if row.warehouse == wave["warehouse"]:
            key = (row.order_id, row.sku)
            available[key] = available.get(key, 0) + row.reserved
    for line in wave["lines"]:
        require(available.get((line["order_id"], line["sku"]), 0) >= line["quantity"],
                "wave_reservation_changed", "Wave units are no longer reserved", 409)


def start_wave(state, context, wave_id, body):
    wave = lookup(state.logistics_waves, context.tenant, wave_id, "wave")
    require(wave["status"] == "planned", "wave_state", "Only planned waves can be started", 409)
    validate_reservations(state, context.tenant, wave)
    wave["picker"] = text(body.get("picker", context.actor), "picker")
    wave["status"] = "picking"
    record(state, context, "logistics.wave.start", wave_id)
    return public(wave)


def pick_line(state, context, wave_id, body):
    wave = lookup(state.logistics_waves, context.tenant, wave_id, "wave")
    require(wave["status"] == "picking", "wave_state", "Wave is not being picked", 409)
    line = next((row for row in wave["lines"] if row["order_id"] == body.get("order_id")
                 and row["sku"] == body.get("sku")), None)
    require(line is not None, "wave_line_not_found", "Unknown wave line", 404)
    count = integer(body.get("quantity"), "quantity", 1, 10000)
    require(line["picked"] + count <= line["quantity"], "overpick", "Picked units exceed wave allocation", 409)
    validate_reservations(state, context.tenant, wave)
    line["picked"] += count
    if all(row["picked"] == row["quantity"] for row in wave["lines"]):
        wave["status"] = "picked"
        publish(state, context.tenant, "logistics.wave.picked", {"wave_id": wave_id})
    record(state, context, "logistics.wave.pick", wave_id,
           {"order_id": line["order_id"], "sku": line["sku"], "quantity": count})
    return public(wave)


def cancel_wave(state, context, wave_id):
    wave = lookup(state.logistics_waves, context.tenant, wave_id, "wave")
    require(wave["status"] in {"planned", "picking", "picked"},
            "wave_state", "Packed or dispatched waves cannot be cancelled", 409)
    cartons = [row for (owner, _), row in state.logistics_cartons.items()
               if owner == context.tenant and row["wave_id"] == wave_id and row["status"] != "void"]
    require(not cartons, "wave_has_cartons", "Void cartons before cancelling the wave", 409)
    wave["status"] = "cancelled"
    record(state, context, "logistics.wave.cancel", wave_id)
    return public(wave)


def wave_view(state, tenant, wave_id):
    wave = lookup(state.logistics_waves, tenant, wave_id, "wave")
    result = public(wave)
    result["picked_units"] = sum(row["picked"] for row in wave["lines"])
    result["carton_ids"] = sorted(identity for (owner, identity), row in state.logistics_cartons.items()
                                 if owner == tenant and row["wave_id"] == wave_id and row["status"] != "void")
    return result
