from dataclasses import dataclass, field


@dataclass
class Plan:
    tenant: str
    plan_id: str
    name: str
    items: list
    schedule: dict
    maximum_multiplier: int
    retry_days: int
    maximum_attempts: int
    active: bool = True
    version: int = 1


@dataclass
class Subscription:
    tenant: str
    subscription_id: str
    customer_id: str
    enrollment_key: str
    enrollment_fingerprint: dict
    plan_id: str
    plan_version: int
    items: list
    schedule: dict
    start_date: str
    multiplier: int
    checkout: dict
    retry_days: int
    maximum_attempts: int
    status: str = "active"
    next_index: int = 0
    version: int = 1
    pauses: list = field(default_factory=list)
    skips: dict = field(default_factory=dict)
    pending_change: dict | None = None
    stop_reason: str | None = None


@dataclass
class Cycle:
    tenant: str
    cycle_id: str
    subscription_id: str
    customer_id: str
    index: int
    due_date: str
    plan_id: str
    plan_version: int
    items: list
    checkout: dict
    retry_days: int
    maximum_attempts: int
    status: str = "planned"
    skip_reason: str | None = None
    order_id: str | None = None
    attempts: list = field(default_factory=list)
    next_attempt_date: str | None = None
    hold_reason: str | None = None
    version: int = 1
