from dataclasses import dataclass
from meridian.core.errors import require


@dataclass(frozen=True)
class Context:
    tenant: str
    actor: str
    roles: frozenset
    providers: frozenset = frozenset()


def authenticate(settings, credential):
    context = settings.credentials.get(credential)
    require(context is not None, "unauthenticated", "A configured credential is required", 401)
    return context


def authorize(context, roles):
    require(bool(context.roles.intersection(roles)), "forbidden", "Role cannot use this route", 403)


def authorize_provider(context, provider):
    require(provider in context.providers, "provider_forbidden", "Credential cannot submit for this provider", 403)
