from meridian.core.money import cents
from meridian.core.errors import require
from meridian.billing.invoices import invoice_view
from meridian.events.outbox import publish


def issue_credit(state, tenant, order_id, amount, reason):
    amount = cents(amount, allow_zero=True)
    invoice = invoice_view(state, tenant, order_id)
    require(amount <= invoice["total_cents"] - invoice["credited_cents"],
            "credit_exceeds_invoice", "Credits cannot exceed the original invoice", 409)
    credit = {"credit_id": state.next_id(tenant, "credit"), "order_id": order_id,
              "amount_cents": amount, "reason": reason}
    state.credits[(tenant, credit["credit_id"])] = credit
    publish(state, tenant, "credit.issued", credit)
    return credit


def order_credits(state, tenant, order_id):
    return [dict(row) for (owner, _), row in state.credits.items()
            if owner == tenant and row["order_id"] == order_id]
