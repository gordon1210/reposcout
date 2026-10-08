from dataclasses import dataclass


@dataclass
class StockBin:
    tenant: str
    warehouse: str
    sku: str
    on_hand: int = 0


@dataclass
class Reservation:
    tenant: str
    reservation_id: str
    order_id: str
    sku: str
    warehouse: str
    reserved: int
    shipped: int = 0
    released: int = 0

    def public(self):
        return {"reservation_id": self.reservation_id, "warehouse": self.warehouse,
                "sku": self.sku, "reserved": self.reserved,
                "shipped": self.shipped, "released": self.released}
