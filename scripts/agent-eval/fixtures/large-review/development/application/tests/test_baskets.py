from tests.support import CommerceTest


class BasketTests(CommerceTest):
    def test_edits_require_current_version_and_zero_removes(self):
        basket = self.ok("POST", "/baskets")
        path = f"/baskets/{basket['basket_id']}/items"
        updated = self.ok("PUT", path, {"sku": "TEA", "quantity": 2, "version": 1})
        self.assertEqual(updated["version"], 2)
        self.assertEqual(self.raw("PUT", path, {"sku": "TEA", "quantity": 3, "version": 1})["status"], 409)
        removed = self.ok("PUT", path, {"sku": "TEA", "quantity": 0, "version": 2})
        self.assertEqual(removed["items"], [])

    def test_other_tenant_cannot_read_basket(self):
        basket = self.ok("POST", "/baskets")
        self.assertEqual(self.raw("GET", f"/baskets/{basket['basket_id']}", token="b-customer")["status"], 404)
