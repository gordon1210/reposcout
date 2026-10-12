from meridian.core.money import cents
from meridian.core.identity import identifier
from meridian.core.errors import require
from meridian.billing.invoices import invoice_view
from meridian.events.outbox import publish


def capture_payment(state, tenant, order_id, amount, payment_reference):
    amount = cents(amount)
    reference = identifier(payment_reference, "payment_reference")
    key = (tenant, reference)
    if key in state.payments:
        previous = state.payments[key]
        require(previous["order_id"] == order_id and previous["amount_cents"] == amount,
                "payment_reference_conflict", "Payment reference was used for another charge", 409)
        return previous
    invoice = invoice_view(state, tenant, order_id)
    require(amount <= invoice["due_cents"], "overpayment", "Payment exceeds the open balance", 409)
    payment = {"payment_reference": reference, "order_id": order_id, "amount_cents": amount}
    state.payments[key] = payment
    publish(state, tenant, "payment.captured", payment)
    return payment
