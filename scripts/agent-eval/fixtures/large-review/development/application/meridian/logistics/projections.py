"""Customer delivery cards and warehouse metrics from current operational facts."""
from meridian.core.errors import require
from meridian.orders.repository import get_order
from meridian.logistics.common import integer, public


def customer_shipments(state, context, order_id):
    order = get_order(state, context.tenant, order_id)
    require(order.customer_id == context.actor, "order_not_found", "Unknown order", 404)
    parcels = []
    for (owner, _), carton in sorted(state.logistics_cartons.items()):
        if owner != context.tenant or carton["order_id"] != order_id or carton["status"] != "dispatched":
            continue
        manifest = state.logistics_manifests[(owner, carton["manifest_id"])]
        shipments = [state.shipments[(owner, identity)] for identity in carton["shipment_ids"]]
        exceptions = [row for (tenant, _), row in state.logistics_exceptions.items()
                      if tenant == owner and row["carton_id"] == carton["carton_id"]]
        open_exceptions = [row for row in exceptions if row["status"] == "open"]
        delivered = all(row["delivered"] for row in shipments)
        latest_resolution = max((row for row in exceptions if row["status"] == "resolved"),
                                key=lambda row: (row["resolved_tick"], row["exception_id"]), default=None)
        if delivered:
            status = "delivered"
        elif open_exceptions:
            status = "exception"
        elif latest_resolution and latest_resolution["resolution"] in {"returned", "claim"}:
            status = "returned" if latest_resolution["resolution"] == "returned" else "lost_or_damaged"
        else:
            status = "in_transit"
        parcels.append({"carton_id": carton["carton_id"], "status": status,
                        "courier": manifest["service"]["courier"], "service": manifest["service"]["name"],
                        "shipping_date": manifest["shipping_date"], "transit_days": manifest["service"]["transit_days"],
                        "items": [{"sku": row["sku"], "quantity": row["quantity"]} for row in carton["items"]],
                        "shipments": [{"shipment_id": row["shipment_id"], "sku": row["sku"],
                                       "quantity": row["quantity"], "tracking": public(row)["tracking"],
                                       "delivered": row["delivered"]} for row in shipments],
                        "exceptions": [{"kind": row["kind"], "status": row["status"],
                                        "resolution": row["resolution"]} for row in exceptions]})
    assigned = {row["shipment_id"] for parcel in parcels for row in parcel["shipments"]}
    loose = [{"shipment_id": row["shipment_id"], "sku": row["sku"], "quantity": row["quantity"],
              "delivered": row["delivered"], "tracking": public(row)["tracking"]}
             for (owner, identity), row in sorted(state.shipments.items())
             if owner == context.tenant and row["order_id"] == order_id and identity not in assigned]
    return {"order_id": order_id, "parcels": parcels, "other_shipments": loose,
            "shipped_units": sum(line.shipped for line in order.lines.values()),
            "outstanding_units": sum(line.outstanding for line in order.lines.values())}


def operations_summary(state, tenant, body):
    now = integer(body.get("tick", 0), "tick", 0, 1000000000)
    waves = [row for (owner, _), row in state.logistics_waves.items() if owner == tenant]
    cartons = [row for (owner, _), row in state.logistics_cartons.items() if owner == tenant and row["status"] != "void"]
    manifests = [row for (owner, _), row in state.logistics_manifests.items() if owner == tenant]
    exceptions = [row for (owner, _), row in state.logistics_exceptions.items() if owner == tenant]
    claims = [row for (owner, _), row in state.logistics_claims.items() if owner == tenant]
    open_exceptions = [row for row in exceptions if row["status"] == "open"]
    return {"wave_counts": {status: sum(row["status"] == status for row in waves)
                            for status in ("planned", "picking", "picked", "packed", "dispatched", "cancelled")},
            "carton_counts": {status: sum(row["status"] == status for row in cartons)
                              for status in ("open", "sealed", "dispatched")},
            "dispatch_spend_cents": sum(row["total_cents"] for row in manifests if row["status"] == "dispatched"),
            "open_exceptions": len(open_exceptions),
            "oldest_exception_age": max((max(0, now - row["opened_tick"]) for row in open_exceptions), default=0),
            "claims_requested_cents": sum(row["requested_cents"] for row in claims),
            "claims_approved_cents": sum(row["approved_cents"] for row in claims),
            "claims_recovered_cents": sum(row["paid_cents"] for row in claims),
            "claims_receivable_cents": sum(row["approved_cents"] - row["paid_cents"] for row in claims)}
