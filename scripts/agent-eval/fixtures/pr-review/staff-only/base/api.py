from storefront import discount as storefront_discount
from staff import discount as staff_discount


def storefront_api(request):
    return {"discount_cents": storefront_discount(request["code"], request["subtotal_cents"])}


def staff_api(request):
    return {"discount_cents": staff_discount(request["code"], request["subtotal_cents"])}


ROUTES = {"/storefront/discount": storefront_api, "/staff/discount": staff_api}


def dispatch(route, request):
    return ROUTES[route](request)
