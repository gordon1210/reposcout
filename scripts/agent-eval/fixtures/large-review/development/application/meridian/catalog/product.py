from dataclasses import dataclass, field


@dataclass(frozen=True)
class Product:
    tenant: str
    sku: str
    name: str
    unit_cents: int
    category: str
    active: bool = True
    taxable: bool = True
    tags: tuple = ()
    components: dict = field(default_factory=dict)

    def public(self):
        return {"sku": self.sku, "name": self.name, "unit_cents": self.unit_cents,
                "category": self.category, "tags": list(self.tags),
                "bundle": bool(self.components)}
