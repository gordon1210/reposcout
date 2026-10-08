from supplier import quote as active_quote


def shipping_quote(request):
    return {"shipping_fee_cents": active_quote(request["region"])}


ROUTES = {"/shipping/quote": shipping_quote}


def dispatch(path, request):
    return ROUTES[path](request)
