from meridian.events.envelope import parse_envelope
from meridian.events.identity import event_identity
from meridian.events.inbox import prior_receipt, remember_applied
from meridian.events.registry import event_handler


def route_event(app, tenant, provider, body):
    envelope = parse_envelope(tenant, provider, body)
    handler = event_handler(envelope.event_type)
    key = event_identity(envelope)
    if prior_receipt(app.state, key) is not None:
        return {"applied": False, "duplicate": True, "event_id": envelope.event_id}
    outcome = handler(app, envelope)
    remember_applied(app.state, key, envelope, outcome)
    return {"applied": True, "duplicate": False, "event_id": envelope.event_id, "outcome": outcome}
