from meridian.api.auth import authorize_provider
from meridian.events.routing import route_event


def ingest(app, context, body, params):
    provider = params["provider"]
    authorize_provider(context, provider)
    return route_event(app, context.tenant, provider, body)
