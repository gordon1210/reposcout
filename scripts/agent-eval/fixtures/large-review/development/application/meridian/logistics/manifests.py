"""Courier handover groups sealed cartons and ships through the stock ledger."""
from copy import deepcopy
from meridian.core.audit import record
from meridian.core.errors import require
from meridian.events.outbox import publish
from meridian.fulfillment.shipments import ship_units
from meridian.logistics.common import day, lookup, public, text, tick, warehouse
from meridian.logistics.rates import service_quote
from meridian.orders.repository import get_order


def create_manifest(state, settings, context, body):
    service_id = text(body.get("service_id"), "service_id", 60)
    service = lookup(state.logistics_services, context.tenant, service_id, "courier_service")
    require(service["active"], "inactive_service", "Courier service is inactive", 409)
    warehouse_id = warehouse(settings, context.tenant, body.get("warehouse"))
    shipping_date = day(body.get("shipping_date"))
    manifest_id = state.next_id(context.tenant, "manifest")
    row = {"manifest_id": manifest_id, "warehouse": warehouse_id, "service_id": service_id,
           "service": deepcopy(service), "shipping_date": shipping_date, "status": "open",
           "cartons": [], "total_cents": 0, "dispatch_tick": None, "handover_reference": None}
    state.logistics_manifests[(context.tenant, manifest_id)] = row
    record(state, context, "logistics.manifest.create", manifest_id)
    return public(row)


def add_carton(state, context, manifest_id, body):
    manifest = lookup(state.logistics_manifests, context.tenant, manifest_id, "manifest")
    require(manifest["status"] == "open", "manifest_state", "Only open manifests can change", 409)
    carton_id = text(body.get("carton_id"), "carton_id")
    carton = lookup(state.logistics_cartons, context.tenant, carton_id, "carton")
    require(carton["status"] == "sealed" and carton["manifest_id"] is None,
            "carton_state", "Carton must be sealed and unassigned", 409)
    require(carton["warehouse"] == manifest["warehouse"], "manifest_warehouse",
            "Carton belongs to another warehouse", 409)
    wave = lookup(state.logistics_waves, context.tenant, carton["wave_id"], "wave")
    require(wave["status"] == "packed", "wave_state", "Complete the wave packing first", 409)
    order = get_order(state, context.tenant, carton["order_id"])
    quote = service_quote(manifest["service"], carton, order.region)
    require(quote["eligible"], "courier_constraints", "Carton does not meet courier constraints", 409)
    manifest["cartons"].append({"carton_id": carton_id, "quote": quote})
    manifest["total_cents"] += quote["total_cents"]
    carton["manifest_id"] = manifest_id
    record(state, context, "logistics.manifest.add", manifest_id, {"carton_id": carton_id})
    return public(manifest)


def remove_carton(state, context, manifest_id, carton_id):
    manifest = lookup(state.logistics_manifests, context.tenant, manifest_id, "manifest")
    require(manifest["status"] == "open", "manifest_state", "Only open manifests can change", 409)
    entry = next((row for row in manifest["cartons"] if row["carton_id"] == carton_id), None)
    require(entry is not None, "manifest_carton_not_found", "Carton is not on this manifest", 404)
    carton = lookup(state.logistics_cartons, context.tenant, carton_id, "carton")
    carton["manifest_id"] = None
    manifest["cartons"].remove(entry)
    manifest["total_cents"] -= entry["quote"]["total_cents"]
    record(state, context, "logistics.manifest.remove", manifest_id, {"carton_id": carton_id})
    return public(manifest)


def close_manifest(state, context, manifest_id):
    manifest = lookup(state.logistics_manifests, context.tenant, manifest_id, "manifest")
    require(manifest["status"] == "open" and manifest["cartons"],
            "manifest_state", "An open nonempty manifest is required", 409)
    manifest["status"] = "closed"
    record(state, context, "logistics.manifest.close", manifest_id)
    return public(manifest)


def dispatch_manifest(state, settings, context, manifest_id, body):
    manifest = lookup(state.logistics_manifests, context.tenant, manifest_id, "manifest")
    require(manifest["status"] == "closed", "manifest_state", "Close the manifest before dispatch", 409)
    handover = text(body.get("handover_reference"), "handover_reference")
    dispatched_at = tick(body.get("tick"))
    require(not any(owner == context.tenant and row["handover_reference"] == handover
                    for (owner, _), row in state.logistics_manifests.items()),
            "duplicate_handover", "Handover reference already used", 409)
    touched_waves = set()
    for entry in manifest["cartons"]:
        carton = lookup(state.logistics_cartons, context.tenant, entry["carton_id"], "carton")
        require(carton["status"] == "sealed" and carton["manifest_id"] == manifest_id,
                "manifest_carton_changed", "A manifested carton has changed", 409)
        for item in carton["items"]:
            shipment = ship_units(state, settings, context, carton["order_id"],
                                  {"sku": item["sku"], "quantity": item["quantity"],
                                   "warehouse": manifest["warehouse"]})
            carton["shipment_ids"].append(shipment["shipment_id"])
        carton["status"] = "dispatched"
        touched_waves.add(carton["wave_id"])
    manifest.update(status="dispatched", dispatch_tick=dispatched_at, handover_reference=handover)
    for wave_id in touched_waves:
        cartons = [row for (owner, _), row in state.logistics_cartons.items()
                   if owner == context.tenant and row["wave_id"] == wave_id and row["status"] != "void"]
        if all(row["status"] == "dispatched" for row in cartons):
            state.logistics_waves[(context.tenant, wave_id)]["status"] = "dispatched"
    publish(state, context.tenant, "logistics.manifest.dispatched", {"manifest_id": manifest_id,
            "carton_count": len(manifest["cartons"]), "total_cents": manifest["total_cents"]})
    record(state, context, "logistics.manifest.dispatch", manifest_id, {"handover_reference": handover})
    return public(manifest)


def manifest_document(state, tenant, manifest_id):
    manifest = lookup(state.logistics_manifests, tenant, manifest_id, "manifest")
    parcels = []
    for entry in manifest["cartons"]:
        carton = lookup(state.logistics_cartons, tenant, entry["carton_id"], "carton")
        parcels.append({"carton_id": carton["carton_id"], "order_id": carton["order_id"],
                        "seal": carton["seal"], "measured_grams": carton["measured_grams"],
                        "declared_cents": carton["declared_cents"], "charge_cents": entry["quote"]["total_cents"],
                        "shipment_ids": list(carton["shipment_ids"])})
    return {"manifest_id": manifest_id, "status": manifest["status"], "courier": manifest["service"]["courier"],
            "service_id": manifest["service_id"], "warehouse": manifest["warehouse"],
            "shipping_date": manifest["shipping_date"], "parcels": parcels,
            "parcel_count": len(parcels), "total_grams": sum(row["measured_grams"] for row in parcels),
            "total_declared_cents": sum(row["declared_cents"] for row in parcels),
            "total_cents": manifest["total_cents"], "handover_reference": manifest["handover_reference"]}
