from meridian.core.errors import require
from meridian.orders.repository import get_order


def add_note(state, context, order_id, text, internal=False):
    require(isinstance(text, str) and 1 <= len(text.strip()) <= 1000,
            "invalid_note", "A note needs 1..1000 characters")
    order = get_order(state, context.tenant, order_id)
    note = {"number": len(order.notes) + 1, "actor": context.actor,
            "text": text.strip(), "internal": bool(internal)}
    order.notes.append(note)
    order.touch()
    return dict(note)


def customer_notes(order):
    return [dict(note) for note in order.notes if not note["internal"]]
