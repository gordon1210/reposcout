from shipping.rules import fee
from shipping.render import render_fee


def quote(request):
    return render_fee(fee(request["delivery_pass"]))
