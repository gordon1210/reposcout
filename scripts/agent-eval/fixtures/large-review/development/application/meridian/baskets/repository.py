from meridian.core.errors import require


def get_basket(state, tenant, customer_id, basket_id, editable=False):
    basket = state.baskets.get((tenant, basket_id))
    require(basket is not None and basket.customer_id == customer_id,
            "basket_not_found", "Basket does not exist", 404)
    require(not editable or basket.checked_out_order is None,
            "basket_checked_out", "Basket has already been checked out", 409)
    return basket


def verify_version(basket, expected):
    require(type(expected) is int and expected == basket.version,
            "stale_basket", "Reload the basket before changing it", 409)
