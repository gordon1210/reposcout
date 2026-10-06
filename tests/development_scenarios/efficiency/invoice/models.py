from dataclasses import dataclass


@dataclass(frozen=True)
class Invoice:
    amount_cents: int
    discount_bps: int
    customer_id: str = "guest"
    currency: str = "EUR"
