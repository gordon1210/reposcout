from shipping.handler import quote

HANDLERS = {"quote": quote}
ROUTES = {"POST /shipping/quote": "quote"}


def dispatch(route, request):
    name = ROUTES[route]
    return HANDLERS[name](request)
