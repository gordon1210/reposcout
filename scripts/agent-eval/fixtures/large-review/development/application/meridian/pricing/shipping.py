from meridian.core.errors import require

FEES = {"domestic": 499, "international": 1299}


def shipping_fee(region, subtotal, express=False, pickup=False):
    require(region in FEES, "unsupported_region", "Unknown delivery region")
    require(type(express) is bool and type(pickup) is bool,
            "invalid_delivery", "Delivery flags must be booleans")
    require(not (pickup and express), "invalid_delivery", "Pickup cannot be express")
    if pickup:
        return 0
    ordinary = 0 if subtotal >= 10000 and region == "domestic" else FEES[region]
    return ordinary + (700 if express else 0)
