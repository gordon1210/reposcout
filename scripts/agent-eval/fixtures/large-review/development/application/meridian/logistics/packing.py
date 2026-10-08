"""Packing profiles and verified cartons; packing does not ship inventory."""
from meridian.catalog.repository import get_product
from meridian.core.audit import record
from meridian.core.errors import require
from meridian.logistics.common import boolean, integer, items, lookup, public, text
from meridian.logistics.waves import validate_reservations


def set_profile(state, context, sku, body):
    product = get_product(state, context.tenant, sku, include_inactive=True)
    require(not product.components, "bundle_profile", "Use physical component profiles")
    row = {"sku": sku, "weight_grams": integer(body.get("weight_grams"), "weight_grams", 1),
           "volume_cm3": integer(body.get("volume_cm3"), "volume_cm3", 1),
           "fragile": boolean(body.get("fragile", False), "fragile"),
           "hazardous": boolean(body.get("hazardous", False), "hazardous"),
           "declared_unit_cents": integer(body.get("declared_unit_cents", product.unit_cents), "declared_unit_cents")}
    state.logistics_profiles[(context.tenant, sku)] = row
    record(state, context, "logistics.profile.set", sku)
    return public(row)


def packed_quantities(state, tenant, wave_id, order_id, excluding=None):
    totals = {}
    for (owner, identity), carton in state.logistics_cartons.items():
        if (owner != tenant or identity == excluding or carton["wave_id"] != wave_id
                or carton["order_id"] != order_id or carton["status"] == "void"):
            continue
        for row in carton["items"]:
            totals[row["sku"]] = totals.get(row["sku"], 0) + row["quantity"]
    return totals


def create_carton(state, context, wave_id, body):
    wave = lookup(state.logistics_waves, context.tenant, wave_id, "wave")
    require(wave["status"] == "picked", "wave_state", "Complete picking before packing", 409)
    validate_reservations(state, context.tenant, wave)
    order_id = text(body.get("order_id"), "order_id")
    wanted = items(body.get("items"))
    allocated = {row["sku"]: row["quantity"] for row in wave["lines"] if row["order_id"] == order_id}
    require(allocated, "wave_order_not_found", "Order is not in this wave", 404)
    packed = packed_quantities(state, context.tenant, wave_id, order_id)
    for sku, units in wanted.items():
        require(units + packed.get(sku, 0) <= allocated.get(sku, 0),
                "carton_overpack", "Cartons exceed picked units for this order", 409)
    dimensions = body.get("dimensions_cm")
    require(isinstance(dimensions, list) and len(dimensions) == 3,
            "invalid_dimensions", "Supply three carton dimensions in centimeters")
    dimensions = [integer(value, "dimension", 1, 200) for value in dimensions]
    tare = integer(body.get("tare_grams", 100), "tare_grams", 0, 20000)
    weight, volume, value = tare, 0, 0
    fragile, hazardous = False, False
    content = []
    for sku, count in sorted(wanted.items()):
        profile = lookup(state.logistics_profiles, context.tenant, sku, "packing_profile")
        weight += profile["weight_grams"] * count
        volume += profile["volume_cm3"] * count
        value += profile["declared_unit_cents"] * count
        fragile = fragile or profile["fragile"]
        hazardous = hazardous or profile["hazardous"]
        content.append({"sku": sku, "quantity": count, "unit_weight_grams": profile["weight_grams"],
                        "declared_unit_cents": profile["declared_unit_cents"]})
    require(volume <= dimensions[0] * dimensions[1] * dimensions[2],
            "carton_volume_exceeded", "Contents exceed the carton volume", 409)
    carton_id = state.next_id(context.tenant, "carton")
    carton = {"carton_id": carton_id, "wave_id": wave_id, "order_id": order_id,
              "warehouse": wave["warehouse"], "status": "open", "items": content,
              "dimensions_cm": dimensions, "weight_grams": weight, "tare_grams": tare,
              "contents_volume_cm3": volume, "declared_cents": value,
              "fragile": fragile, "hazardous": hazardous, "seal": None,
              "manifest_id": None, "shipment_ids": []}
    state.logistics_cartons[(context.tenant, carton_id)] = carton
    record(state, context, "logistics.carton.create", carton_id, {"wave_id": wave_id})
    return public(carton)


def seal_carton(state, context, carton_id, body):
    carton = lookup(state.logistics_cartons, context.tenant, carton_id, "carton")
    require(carton["status"] == "open", "carton_state", "Only open cartons can be sealed", 409)
    measured = integer(body.get("measured_grams"), "measured_grams", 1)
    tolerance = max(20, carton["weight_grams"] // 50)
    require(abs(measured - carton["weight_grams"]) <= tolerance,
            "carton_weight_mismatch", "Measured weight is outside the two-percent or twenty-gram tolerance", 409)
    seal = text(body.get("seal"), "seal", 80)
    require(not any(owner == context.tenant and row["seal"] == seal and row["status"] != "void"
                    for (owner, _), row in state.logistics_cartons.items()),
            "duplicate_carton_seal", "Seal is already assigned to a carton", 409)
    carton.update(status="sealed", seal=seal, measured_grams=measured)
    record(state, context, "logistics.carton.seal", carton_id)
    return public(carton)


def void_carton(state, context, carton_id):
    carton = lookup(state.logistics_cartons, context.tenant, carton_id, "carton")
    require(carton["status"] in {"open", "sealed"} and carton["manifest_id"] is None,
            "carton_state", "Only unmanifested cartons can be voided", 409)
    wave = lookup(state.logistics_waves, context.tenant, carton["wave_id"], "wave")
    require(wave["status"] == "picked", "wave_state", "Completed packing cannot be reopened by voiding a carton", 409)
    carton["status"] = "void"
    record(state, context, "logistics.carton.void", carton_id)
    return public(carton)


def complete_packing(state, context, wave_id):
    wave = lookup(state.logistics_waves, context.tenant, wave_id, "wave")
    require(wave["status"] == "picked", "wave_state", "Wave must be picked", 409)
    cartons = [row for (owner, _), row in state.logistics_cartons.items()
               if owner == context.tenant and row["wave_id"] == wave_id and row["status"] != "void"]
    require(cartons and all(row["status"] == "sealed" for row in cartons),
            "unsealed_cartons", "Every carton must be sealed", 409)
    for order_id in wave["order_ids"]:
        expected = {row["sku"]: row["quantity"] for row in wave["lines"] if row["order_id"] == order_id}
        require(packed_quantities(state, context.tenant, wave_id, order_id) == expected,
                "incomplete_packing", "All picked units must be packed", 409)
    validate_reservations(state, context.tenant, wave)
    wave["status"] = "packed"
    record(state, context, "logistics.wave.pack", wave_id, {"cartons": len(cartons)})
    return public(wave)
