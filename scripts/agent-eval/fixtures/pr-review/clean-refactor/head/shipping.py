BASE_FEES = {"domestic": 499, "international": 999}


def quote(region, express=False):
    if region not in BASE_FEES:
        raise ValueError("unknown region")
    return BASE_FEES[region] + (300 if express else 0)


def endpoint(request):
    return {"shipping_fee_cents": quote(request["region"], request.get("express", False))}
