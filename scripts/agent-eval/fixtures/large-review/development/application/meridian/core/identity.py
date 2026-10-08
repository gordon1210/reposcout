import re
from meridian.core.errors import require

IDENTIFIER = re.compile(r"[A-Za-z0-9][A-Za-z0-9_.:-]{0,79}\Z")


def identifier(value, field="id"):
    require(isinstance(value, str) and IDENTIFIER.fullmatch(value) is not None,
            "invalid_identifier", f"{field} must be a bounded identifier")
    return value


def tenant_key(tenant, identifier_value):
    return identifier(tenant, "tenant"), identifier(identifier_value)
