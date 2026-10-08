from copy import deepcopy
from meridian.api.auth import authenticate, authorize
from meridian.api.validation import request_body
from meridian.api.routes import resolve_route
from meridian.core.errors import DomainError
from meridian.core.transaction import atomic


def dispatch(app, method, path, credential, body=None):
    try:
        context = authenticate(app.settings, credential)
        route, params = resolve_route(method.upper(), path)
        authorize(context, route.roles)
        data = request_body(body)
        with atomic(app.state):
            result = route.handler(app, context, data, params)
        return {"status": 200, "data": deepcopy(result)}
    except DomainError as error:
        return {"status": error.status, "error": {"code": error.code, "message": error.message}}
