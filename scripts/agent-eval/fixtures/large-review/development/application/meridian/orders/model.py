from dataclasses import dataclass, field


@dataclass
class OrderLine:
    sku: str
    ordered: int
    unit_cents: int
    discount_cents: int = 0
    tax_cents: int = 0
    shipped: int = 0
    cancelled: int = 0
    returned: int = 0

    @property
    def outstanding(self):
        return self.ordered - self.shipped - self.cancelled


@dataclass
class Order:
    tenant: str
    order_id: str
    customer_id: str
    lines: dict
    shipping_cents: int
    region: str
    allow_backorder: bool = False
    notes: list = field(default_factory=list)
    version: int = 1

    def touch(self):
        self.version += 1
