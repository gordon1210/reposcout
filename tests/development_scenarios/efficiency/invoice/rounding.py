def rounded_discount(amount: int, bps: int) -> int:
    """Round a nonnegative cent amount's basis-point discount to cents."""
    if amount < 0 or not 0 <= bps <= 10000:
        raise ValueError("discount requires cents >= 0 and basis points in [0, 10000]")
    return round(amount * bps / 10000)
