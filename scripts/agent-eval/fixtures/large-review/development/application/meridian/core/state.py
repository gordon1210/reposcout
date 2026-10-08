from dataclasses import dataclass, field


@dataclass
class State:
    products: dict = field(default_factory=dict)
    customers: dict = field(default_factory=dict)
    baskets: dict = field(default_factory=dict)
    orders: dict = field(default_factory=dict)
    stock: dict = field(default_factory=dict)
    reservations: dict = field(default_factory=dict)
    shipments: dict = field(default_factory=dict)
    returns: dict = field(default_factory=dict)
    invoices: dict = field(default_factory=dict)
    payments: dict = field(default_factory=dict)
    credits: dict = field(default_factory=dict)
    inbox: dict = field(default_factory=dict)
    outbox: dict = field(default_factory=dict)
    inventory_ledger: list = field(default_factory=list)
    audit: list = field(default_factory=list)
    purchase_suppliers: dict = field(default_factory=dict)
    purchase_terms: dict = field(default_factory=dict)
    purchase_orders: dict = field(default_factory=dict)
    purchase_receipts: dict = field(default_factory=dict)
    purchase_invoices: dict = field(default_factory=dict)
    purchase_credits: dict = field(default_factory=dict)
    replenishment_rules: dict = field(default_factory=dict)
    replenishment_plans: dict = field(default_factory=dict)
    logistics_waves: dict = field(default_factory=dict)
    logistics_profiles: dict = field(default_factory=dict)
    logistics_cartons: dict = field(default_factory=dict)
    logistics_services: dict = field(default_factory=dict)
    logistics_manifests: dict = field(default_factory=dict)
    logistics_exceptions: dict = field(default_factory=dict)
    logistics_claims: dict = field(default_factory=dict)
    recurring_plans: dict = field(default_factory=dict)
    recurring_subscriptions: dict = field(default_factory=dict)
    recurring_cycles: dict = field(default_factory=dict)
    sequences: dict = field(default_factory=dict)

    def next_id(self, tenant, prefix):
        key = (tenant, prefix)
        self.sequences[key] = self.sequences.get(key, 0) + 1
        return f"{prefix}-{self.sequences[key]:04d}"
