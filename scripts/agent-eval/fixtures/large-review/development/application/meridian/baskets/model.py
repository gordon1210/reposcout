from dataclasses import dataclass, field


@dataclass
class Basket:
    tenant: str
    basket_id: str
    customer_id: str
    items: dict = field(default_factory=dict)
    version: int = 1
    checked_out_order: str | None = None

    def public(self):
        return {"basket_id": self.basket_id, "version": self.version,
                "items": [{"sku": sku, "quantity": count} for sku, count in sorted(self.items.items())],
                "checked_out_order": self.checked_out_order}
