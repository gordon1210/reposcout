from meridian.catalog.repository import get_product
from meridian.core.errors import require
from meridian.core.quantities import quantity


def expand_components(state, tenant, sku, count=1, trail=()):
    require(sku not in trail, "bundle_cycle", "Bundle definitions must be acyclic", 409)
    product = get_product(state, tenant, sku)
    count = quantity(count)
    if not product.components:
        return {sku: count}
    result = {}
    for component, units in sorted(product.components.items()):
        for leaf, total in expand_components(state, tenant, component, count * units, trail + (sku,)).items():
            result[leaf] = result.get(leaf, 0) + total
    return result


def bundle_capacity(state, tenant, sku, available):
    requirements = expand_components(state, tenant, sku)
    return min(available(component) // units for component, units in requirements.items())
