def provider_receipt_key(envelope):
    return envelope.provider, envelope.event_id


def event_identity(envelope):
    return envelope.tenant, envelope.provider, envelope.event_id
