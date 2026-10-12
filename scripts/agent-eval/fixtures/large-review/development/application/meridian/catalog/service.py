from dataclasses import replace
from meridian.catalog.product import Product
from meridian.catalog.repository import get_product, save_product
from meridian.core.identity import identifier
from meridian.core.money import cents
from meridian.core.errors import require


def create_product(state, tenant, body):
    sku = identifier(body.get("sku"), "sku")
    require((tenant, sku) not in state.products, "product_exists", "SKU already exists", 409)
    name = body.get("name")
    require(isinstance(name, str) and 1 <= len(name.strip()) <= 120, "invalid_name", "Provide a product name")
    product = Product(tenant, sku, name.strip(), cents(body.get("unit_cents")),
                      identifier(body.get("category", "general"), "category"),
                      tags=tuple(sorted(set(body.get("tags", [])))))
    return save_product(state, product)


def retire_product(state, tenant, sku):
    product = get_product(state, tenant, sku, include_inactive=True)
    return save_product(state, replace(product, active=False))


def reprice_product(state, tenant, sku, amount):
    product = get_product(state, tenant, sku, include_inactive=True)
    return save_product(state, replace(product, unit_cents=cents(amount)))
