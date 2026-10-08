from meridian.core.errors import require


def get_product(state, tenant, sku, include_inactive=False):
    product = state.products.get((tenant, sku))
    require(product is not None, "product_not_found", f"Unknown product {sku}", 404)
    require(include_inactive or product.active, "product_inactive", "Product is not for sale", 409)
    return product


def save_product(state, product):
    state.products[(product.tenant, product.sku)] = product
    return product


def list_products(state, tenant):
    return [product for (owner, _), product in state.products.items() if owner == tenant]
