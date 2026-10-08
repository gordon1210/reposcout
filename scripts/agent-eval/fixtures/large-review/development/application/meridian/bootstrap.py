from meridian.app import Application
from meridian.catalog.product import Product
from meridian.catalog.repository import save_product
from meridian.customers.profile import Customer
from meridian.customers.address import Address


def demo_application():
    app = Application()
    for tenant, customer_id in (("tenant-a", "alice"), ("tenant-b", "bob")):
        products = (
            Product(tenant, "TEA", "Breakfast tea", 1250, "grocery", taxable=False, tags=("tea", "breakfast")),
            Product(tenant, "MUG", "Stoneware mug", 900, "home", tags=("ceramic",)),
            Product(tenant, "KIT", "Tea starter kit", 2150, "gifts", components={"TEA": 1, "MUG": 1}),
            Product(tenant, "CASE", "Wholesale tea case", 8000, "grocery", taxable=False, tags=("wholesale",)),
        )
        for product in products:
            save_product(app.state, product)
        customer = Customer(tenant, customer_id, f"{customer_id}@example.test",
                            Address(customer_id.title(), "Example street 1", "10115", "DE", "domestic"))
        app.state.customers[(tenant, customer_id)] = customer
    return app
