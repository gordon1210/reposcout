from dataclasses import dataclass, field
from meridian.customers.address import Address


@dataclass
class Customer:
    tenant: str
    customer_id: str
    email: str
    address: Address
    wholesale: bool = False
    completed_orders: int = 0
    preferences: dict = field(default_factory=lambda: {"email_receipts": True, "language": "en"})

    def public(self):
        return {"customer_id": self.customer_id, "email": self.email,
                "wholesale": self.wholesale, "completed_orders": self.completed_orders,
                "preferences": dict(self.preferences), "country": self.address.country}
