from meridian.core.money import cents
from meridian.api.validation import optional_text


def promotion_preview(app, context, body, params):
    subtotal = cents(body.get("subtotal_cents"), allow_zero=True)
    code = optional_text(body, "code") or ""
    eligible = code.strip().upper().startswith("WELCOME")
    reduction = min(subtotal, 500) if eligible else 0
    return {"preview": True, "eligible": eligible, "discount_cents": reduction,
            "estimated_cents": subtotal - reduction}
