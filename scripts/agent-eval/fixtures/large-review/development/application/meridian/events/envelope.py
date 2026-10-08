from dataclasses import dataclass
from meridian.core.errors import require
from meridian.core.identity import identifier


@dataclass(frozen=True)
class Envelope:
    tenant: str
    provider: str
    event_id: str
    event_type: str
    data: dict


def parse_envelope(tenant, provider, body):
    require(isinstance(body, dict), "invalid_event", "Event must be an object")
    event_id = identifier(body.get("event_id"), "event_id")
    event_type = identifier(body.get("type"), "type")
    data = body.get("data")
    require(isinstance(data, dict), "invalid_event", "Event data must be an object")
    return Envelope(tenant, provider, event_id, event_type, dict(data))
