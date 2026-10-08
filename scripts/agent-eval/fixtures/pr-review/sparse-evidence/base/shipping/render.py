def render_fee(fee_cents):
    return {"shipping_fee_cents": 499 if fee_cents is None else fee_cents}
