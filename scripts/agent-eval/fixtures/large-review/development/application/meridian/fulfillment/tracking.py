from meridian.core.errors import require

STATUSES = {"collected": 1, "in_transit": 2, "delivered": 3}


def record_tracking(state, tenant, shipment_id, status, location):
    shipment = state.shipments.get((tenant, shipment_id))
    require(shipment is not None, "shipment_not_found", "Unknown shipment", 404)
    require(status in STATUSES, "invalid_tracking_status", "Unknown tracking status")
    require(isinstance(location, str) and 1 <= len(location) <= 120,
            "invalid_location", "Tracking location must be bounded text")
    previous = shipment["tracking"][-1]["status"] if shipment["tracking"] else None
    require(previous is None or STATUSES[status] >= STATUSES[previous],
            "tracking_regression", "Tracking cannot move backwards", 409)
    entry = {"status": status, "location": location}
    if not shipment["tracking"] or shipment["tracking"][-1] != entry:
        shipment["tracking"].append(entry)
    shipment["delivered"] = status == "delivered"
    return {"shipment_id": shipment_id, "tracking": list(shipment["tracking"]), "delivered": shipment["delivered"]}
