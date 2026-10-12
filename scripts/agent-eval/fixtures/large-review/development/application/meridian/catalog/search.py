from meridian.catalog.repository import list_products
from meridian.catalog.visibility import visible
from meridian.core.pagination import page


def search_catalog(state, tenant, customer, request):
    term = str(request.get("query", "")).strip().casefold()
    category = request.get("category")
    rows = [product for product in list_products(state, tenant)
            if visible(product, customer)
            and (not category or product.category == category)
            and (not term or term in (product.sku + " " + product.name + " " + " ".join(product.tags)).casefold())]
    selected, cursor = page(rows, request, lambda row: row.sku)
    return {"products": [row.public() for row in selected], "next_cursor": cursor}
