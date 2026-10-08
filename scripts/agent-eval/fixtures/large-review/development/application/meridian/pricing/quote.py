from meridian.catalog.repository import get_product
from meridian.catalog.visibility import require_visible
from meridian.catalog.bundles import expand_components
from meridian.core.errors import require
from meridian.core.quantities import quantity
from meridian.pricing.promotions import discount_for, apply_discount
from meridian.pricing.tax import taxed_lines
from meridian.pricing.shipping import shipping_fee


def quote(state, settings, tenant, customer, items, options):
    require(isinstance(items, list) and 1 <= len(items) <= 100,
            "invalid_items", "An order needs 1..100 items")
    merged = {}
    for item in items:
        require(isinstance(item, dict), "invalid_item", "Every item must be an object")
        sku = item.get("sku")
        require(isinstance(sku, str), "invalid_sku", "SKU must be text")
        product = require_visible(get_product(state, tenant, sku), customer)
        require(not product.components, "bundle_checkout", "Choose the component items for checkout")
        merged[sku] = quantity(merged.get(sku, 0) + quantity(item.get("quantity")))
    lines = []
    for sku, count in sorted(merged.items()):
        product = get_product(state, tenant, sku)
        lines.append({"sku": sku, "quantity": count, "unit_cents": product.unit_cents,
                      "subtotal_cents": product.unit_cents * count, "taxable": product.taxable})
    subtotal = sum(line["subtotal_cents"] for line in lines)
    discount = discount_for(options.get("promotion"), subtotal, customer)
    region = options.get("region", customer.address.region)
    lines = taxed_lines(apply_discount(lines, discount), region, settings.tax_rates)
    tax = sum(line["tax_cents"] for line in lines)
    shipping = shipping_fee(region, subtotal - discount, options.get("express", False), options.get("pickup", False))
    return {"lines": lines, "subtotal_cents": subtotal, "discount_cents": discount,
            "tax_cents": tax, "shipping_cents": shipping,
            "total_cents": subtotal - discount + tax + shipping, "currency": "EUR", "region": region}
