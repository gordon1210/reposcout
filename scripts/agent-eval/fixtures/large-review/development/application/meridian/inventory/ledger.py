def post_inventory(state, tenant, kind, warehouse, sku, on_hand_delta, reserved_delta,
                   order_id=None, reference=None):
    entry = {"entry_id": state.next_id(tenant, "stock-entry"), "tenant": tenant,
             "kind": kind, "warehouse": warehouse, "sku": sku,
             "on_hand_delta": on_hand_delta, "reserved_delta": reserved_delta,
             "order_id": order_id, "reference": reference}
    state.inventory_ledger.append(entry)
    return entry


def inventory_entries(state, tenant, order_id=None, sku=None, kind=None):
    return [dict(row) for row in state.inventory_ledger
            if row["tenant"] == tenant and (order_id is None or row["order_id"] == order_id)
            and (sku is None or row["sku"] == sku) and (kind is None or row["kind"] == kind)]
