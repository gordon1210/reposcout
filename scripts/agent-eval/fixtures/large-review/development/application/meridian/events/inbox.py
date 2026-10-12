def prior_receipt(state, key):
    return state.inbox.get(key)


def remember_applied(state, key, envelope, outcome):
    receipt = {"tenant": envelope.tenant, "provider": envelope.provider,
               "event_id": envelope.event_id, "type": envelope.event_type,
               "outcome": dict(outcome)}
    state.inbox[key] = receipt
    return receipt


def receipt_summary(state, tenant):
    rows = [dict(value) for value in state.inbox.values() if value["tenant"] == tenant]
    rows.sort(key=lambda row: (row["provider"], row["event_id"]))
    return {"applied_count": len(rows), "receipts": rows}
