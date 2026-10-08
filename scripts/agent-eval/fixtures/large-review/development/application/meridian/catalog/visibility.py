from meridian.core.errors import require


def visible(product, customer):
    if not product.active:
        return False
    if "wholesale" in product.tags and not customer.wholesale:
        return False
    return True


def require_visible(product, customer):
    require(visible(product, customer), "product_unavailable", "Product is unavailable to this account", 404)
    return product
