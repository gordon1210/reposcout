"""Pure courier comparison with dimensional weight and transparent exclusions."""
from meridian.core.errors import require
from meridian.logistics.common import integer, lookup
from meridian.orders.repository import get_order


def ceil_div(numerator, denominator):
    return (numerator + denominator - 1) // denominator


def service_quote(service, carton, region):
    rejected = []
    dimensions = sorted(carton["dimensions_cm"])
    girth = dimensions[2] + 2 * (dimensions[0] + dimensions[1])
    volume = dimensions[0] * dimensions[1] * dimensions[2]
    measured = carton.get("measured_grams", carton["weight_grams"])
    dimensional = ceil_div(volume * 1000, service["dimensional_divisor"])
    chargeable = max(measured, dimensional)
    if not service["active"]:
        rejected.append("inactive")
    if region not in service["regions"]:
        rejected.append("region")
    if chargeable > service["max_weight_grams"]:
        rejected.append("weight")
    if dimensions[2] > service["max_longest_cm"] or girth > service["max_girth_cm"]:
        rejected.append("dimensions")
    if carton["declared_cents"] > service["max_declared_cents"]:
        rejected.append("declared_value")
    if carton["fragile"] and not service["accepts_fragile"]:
        rejected.append("fragile")
    if carton["hazardous"] and not service["accepts_hazardous"]:
        rejected.append("hazardous")
    kilograms = ceil_div(chargeable, 1000)
    transport = service["base_cents"] + kilograms * service["per_kg_cents"]
    fuel = ceil_div(transport * service["fuel_basis_points"], 10000)
    insurance = ceil_div(carton["declared_cents"] * service["insurance_basis_points"], 10000)
    return {"service_id": service["service_id"], "service_version": service["version"],
            "courier": service["courier"], "eligible": not rejected, "reasons": rejected,
            "actual_grams": measured, "dimensional_grams": dimensional,
            "chargeable_grams": chargeable, "billed_kilograms": kilograms,
            "transport_cents": transport, "fuel_cents": fuel, "insurance_cents": insurance,
            "total_cents": transport + fuel + insurance, "transit_days": service["transit_days"]}


def compare_rates(state, tenant, carton_id, body):
    carton = lookup(state.logistics_cartons, tenant, carton_id, "carton")
    require(carton["status"] != "void", "carton_state", "Cannot quote a void carton", 409)
    order = get_order(state, tenant, carton["order_id"])
    deadline = body.get("max_transit_days")
    if deadline is not None:
        deadline = integer(deadline, "max_transit_days", 1, 30)
    quotes = []
    for (owner, _), service in sorted(state.logistics_services.items()):
        if owner != tenant:
            continue
        quote = service_quote(service, carton, order.region)
        if deadline is not None and quote["transit_days"] > deadline:
            quote["reasons"].append("deadline")
            quote["eligible"] = False
        quotes.append(quote)
    eligible = sorted((row for row in quotes if row["eligible"]),
                      key=lambda row: (row["total_cents"], row["transit_days"], row["service_id"]))
    return {"carton_id": carton_id, "region": order.region, "eligible": eligible,
            "excluded": [row for row in quotes if not row["eligible"]],
            "recommended_service_id": eligible[0]["service_id"] if eligible else None}
