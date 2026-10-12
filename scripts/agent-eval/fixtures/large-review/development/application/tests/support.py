import unittest
from meridian.bootstrap import demo_application


class CommerceTest(unittest.TestCase):
    def setUp(self):
        self.app = demo_application()

    def raw(self, method, path, body=None, token="a-customer"):
        return self.app.request(method, path, token, body)

    def ok(self, method, path, body=None, token="a-customer"):
        response = self.raw(method, path, body, token)
        self.assertEqual(response["status"], 200, response)
        return response["data"]

    def receive(self, units, warehouse="north", sku="TEA", tenant="a"):
        return self.ok("POST", "/stock/receive", {"warehouse": warehouse, "sku": sku, "quantity": units}, f"{tenant}-warehouse")

    def stock(self, sku="TEA", tenant="a"):
        return self.ok("GET", f"/stock/{sku}", token=f"{tenant}-warehouse")

    def order(self, units, sku="TEA", tenant="a", **options):
        return self.ok("POST", "/orders", dict(items=[{"sku": sku, "quantity": units}], **options), f"{tenant}-customer")

    def show(self, order_id, tenant="a"):
        return self.ok("GET", f"/orders/{order_id}", token=f"{tenant}-customer")

    def ship(self, order_id, units, sku="TEA", **options):
        return self.ok("POST", f"/warehouse/orders/{order_id}/ship", dict(sku=sku, quantity=units, **options), "a-warehouse")

    def cancel(self, order_id, units, sku="TEA"):
        return self.ok("POST", f"/orders/{order_id}/cancel", {"sku": sku, "quantity": units})

    def event(self, event_id, event_type, data, tenant="a", provider="acme", **extra):
        return self.raw("POST", f"/events/{provider}", dict(event_id=event_id, type=event_type, data=data, **extra), f"{tenant}-provider")
