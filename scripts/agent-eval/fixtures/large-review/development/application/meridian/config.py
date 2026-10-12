from dataclasses import dataclass
from meridian.api.auth import Context


@dataclass(frozen=True)
class Settings:
    credentials: dict
    warehouses: dict
    tax_rates: dict


def default_settings():
    credentials = {}
    for label, tenant, customer in (("a", "tenant-a", "alice"), ("b", "tenant-b", "bob")):
        credentials[f"{label}-customer"] = Context(tenant, customer, frozenset({"customer"}))
        credentials[f"{label}-admin"] = Context(tenant, f"{label}-operator", frozenset({"admin"}))
        credentials[f"{label}-warehouse"] = Context(tenant, f"{label}-packer", frozenset({"warehouse"}))
        credentials[f"{label}-provider"] = Context(tenant, f"{label}-integration", frozenset({"provider"}),
                                                    frozenset({"acme", "backup"}))
    return Settings(credentials, {"tenant-a": ("north", "south"), "tenant-b": ("north", "south")},
                    {"domestic": 1900, "international": 0})
