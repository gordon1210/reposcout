def return_allocations(state, tenant, shipment, units):
    previously_returned = {}
    for (owner, _), entry in state.returns.items():
        if owner == tenant and entry["shipment_id"] == shipment["shipment_id"]:
            for allocation in entry["allocations"]:
                warehouse = allocation["warehouse"]
                previously_returned[warehouse] = previously_returned.get(warehouse, 0) + allocation["quantity"]
    shipped_by_warehouse = {}
    for allocation in shipment["allocations"]:
        warehouse = allocation["warehouse"]
        shipped_by_warehouse[warehouse] = shipped_by_warehouse.get(warehouse, 0) + allocation["quantity"]
    remaining = units
    result = []
    for warehouse, shipped_units in shipped_by_warehouse.items():
        available = shipped_units - previously_returned.get(warehouse, 0)
        selected = min(remaining, available)
        if selected:
            result.append({"warehouse": warehouse, "quantity": selected})
            remaining -= selected
        if not remaining:
            break
    return result
