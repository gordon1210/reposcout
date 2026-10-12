from meridian.core.errors import require
from meridian.core.identity import identifier
from meridian.events.envelope import Envelope
from meridian.events.identity import provider_receipt_key
from meridian.events.registry import event_handler


def reconcile_provider_receipts(state, tenant, provider, body):
    provider = identifier(provider, "provider")
    entries = body.get("events")
    require(isinstance(entries, list) and len(entries) <= 100,
            "invalid_manifest", "A delivery manifest needs a list of at most 100 events")
    declared = {}
    for entry in entries:
        require(isinstance(entry, dict), "invalid_manifest_entry", "Manifest events must be objects")
        event_id = identifier(entry.get("event_id"), "event_id")
        event_type = identifier(entry.get("type"), "type")
        event_handler(event_type)
        envelope = Envelope(tenant, provider, event_id, event_type, {})
        key = provider_receipt_key(envelope)
        require(key not in declared, "duplicate_manifest_event", "Manifest event IDs must be unique")
        declared[key] = envelope
    scoped_receipts = {}
    for receipt in state.inbox.values():
        if receipt["tenant"] == tenant and receipt["provider"] == provider:
            envelope = Envelope(tenant, provider, receipt["event_id"], receipt["type"], {})
            scoped_receipts[provider_receipt_key(envelope)] = receipt
    applied = []
    missing = []
    conflicts = []
    for key, envelope in sorted(declared.items()):
        receipt = scoped_receipts.get(key)
        if receipt is None:
            missing.append(envelope.event_id)
        elif receipt["type"] != envelope.event_type:
            conflicts.append({"event_id": envelope.event_id, "declared_type": envelope.event_type,
                              "applied_type": receipt["type"]})
        else:
            applied.append({"event_id": envelope.event_id, "type": envelope.event_type,
                            "outcome": dict(receipt["outcome"])})
    unlisted = sorted(receipt["event_id"] for key, receipt in scoped_receipts.items() if key not in declared)
    return {"provider": provider, "declared_count": len(declared), "applied": applied,
            "matched_count": len(applied), "missing": missing, "type_conflicts": conflicts,
            "unlisted": unlisted, "complete": not missing and not conflicts}
