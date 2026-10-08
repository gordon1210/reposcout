from calculator import total_cents
from serializer import render_total


def checkout(request):
    return render_total(total_cents(request["items"]))


ROUTES = {"POST /checkout": checkout}


def dispatch(route, request):
    return ROUTES[route](request)
