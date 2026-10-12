from policy import eligible as may_refund


def web_refund(request):
    return {"allowed": may_refund(request["days"])}


def support_refund(request):
    return {"allowed": may_refund(request["days"])}


ROUTES = {"/refund": web_refund, "/support/refund": support_refund}


def dispatch(path, request):
    return ROUTES[path](request)
