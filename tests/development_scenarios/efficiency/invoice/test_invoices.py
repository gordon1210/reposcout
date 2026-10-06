from src.invoices import net_due
from src.models import Invoice


def test_net_due_rounds_half_up():
    assert net_due(Invoice(amount_cents=1000, discount_bps=1000)) == 900
    assert net_due(Invoice(amount_cents=1005, discount_bps=1000)) == 904
    assert net_due(Invoice(amount_cents=1005, discount_bps=0)) == 1005
    assert net_due(Invoice(amount_cents=1005, discount_bps=10000)) == 0
