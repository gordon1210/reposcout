"""Tenant-managed courier contracts, all rates expressed in integer cents."""
from meridian.core.audit import record
from meridian.core.errors import require
from meridian.logistics.common import boolean, integer, lookup, public, text


def register_service(state, context, body):
    service_id = text(body.get("service_id"), "service_id", 60)
    regions = body.get("regions")
    require(isinstance(regions, list) and 1 <= len(regions) <= 20
            and all(isinstance(region, str) and 1 <= len(region) <= 40 for region in regions),
            "invalid_service_regions", "Provide a bounded list of region names")
    require(len(set(regions)) == len(regions), "duplicate_service_region", "Regions must be unique")
    prior = state.logistics_services.get((context.tenant, service_id))
    row = {"service_id": service_id, "courier": text(body.get("courier"), "courier", 80),
           "name": text(body.get("name"), "name"), "regions": sorted(regions),
           "base_cents": integer(body.get("base_cents"), "base_cents"),
           "per_kg_cents": integer(body.get("per_kg_cents"), "per_kg_cents"),
           "fuel_basis_points": integer(body.get("fuel_basis_points", 0), "fuel_basis_points", 0, 10000),
           "insurance_basis_points": integer(body.get("insurance_basis_points", 0), "insurance_basis_points", 0, 10000),
           "max_weight_grams": integer(body.get("max_weight_grams", 30000), "max_weight_grams", 1),
           "max_longest_cm": integer(body.get("max_longest_cm", 120), "max_longest_cm", 1, 200),
           "max_girth_cm": integer(body.get("max_girth_cm", 300), "max_girth_cm", 1, 1000),
           "dimensional_divisor": integer(body.get("dimensional_divisor", 5000), "dimensional_divisor", 1, 100000),
           "max_declared_cents": integer(body.get("max_declared_cents", 100000), "max_declared_cents", 1, 100000000),
           "transit_days": integer(body.get("transit_days", 3), "transit_days", 1, 30),
           "accepts_fragile": boolean(body.get("accepts_fragile", True), "accepts_fragile"),
           "accepts_hazardous": boolean(body.get("accepts_hazardous", False), "accepts_hazardous"),
           "active": boolean(body.get("active", True), "active"),
           "version": prior["version"] + 1 if prior else 1}
    state.logistics_services[(context.tenant, service_id)] = row
    record(state, context, "logistics.service.register", service_id, {"version": row["version"]})
    return public(row)


def service_listing(state, tenant, include_inactive=False):
    return {"services": [public(row) for (owner, _), row in sorted(state.logistics_services.items())
                         if owner == tenant and (include_inactive or row["active"])]}


def retire_service(state, context, service_id):
    row = lookup(state.logistics_services, context.tenant, service_id, "courier_service")
    row["active"] = False
    row["version"] += 1
    record(state, context, "logistics.service.retire", service_id)
    return public(row)
