from meridian.core.errors import require


def request_body(body):
    require(body is None or isinstance(body, dict), "invalid_body", "Request body must be an object")
    return dict(body or {})


def optional_text(body, key, limit=100):
    value = body.get(key)
    require(value is None or (isinstance(value, str) and len(value) <= limit),
            "invalid_text", f"{key} must be text of at most {limit} characters")
    return value


def boolean_option(body, key, default=False):
    value = body.get(key, default)
    require(type(value) is bool, "invalid_boolean", f"{key} must be boolean")
    return value
