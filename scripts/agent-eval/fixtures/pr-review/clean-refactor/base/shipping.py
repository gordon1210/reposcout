def quote(region, express=False):
    if region == "domestic":
        fee = 499
    elif region == "international":
        fee = 999
    else:
        raise ValueError("unknown region")
    return fee + (300 if express else 0)


def endpoint(request):
    return {"shipping_fee_cents": quote(request["region"], request.get("express", False))}
