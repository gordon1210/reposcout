from tests.support import CommerceTest


class CatalogAdminTests(CommerceTest):
    def test_repricing_preserves_placed_invoice(self):
        self.receive(2)
        order = self.order(1, pickup=True)
        self.ok("PUT", "/admin/products/TEA/price", {"unit_cents": 1600}, "a-admin")
        quote = self.ok("POST", "/quotes", {"items": [{"sku": "TEA", "quantity": 1}], "pickup": True})
        self.assertEqual(quote["total_cents"], 1600)
        self.assertEqual(self.show(order["order_id"])["invoice"]["total_cents"], 1250)

    def test_create_and_retire_product(self):
        product = self.ok("POST", "/admin/products", {"sku": "SPOON", "name": "Tea spoon", "unit_cents": 400}, "a-admin")
        self.assertEqual(product["category"], "general")
        self.ok("POST", "/admin/products/SPOON/retire", token="a-admin")
        self.assertEqual(self.raw("GET", "/catalog/SPOON")["status"], 409)
