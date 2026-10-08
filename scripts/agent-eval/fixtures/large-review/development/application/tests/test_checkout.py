from tests.support import CommerceTest


class CheckoutTests(CommerceTest):
    def test_checkout_replay_returns_same_order(self):
        self.receive(3)
        basket = self.ok("POST", "/baskets")
        base = f"/baskets/{basket['basket_id']}"
        self.ok("PUT", base + "/items", {"sku": "TEA", "quantity": 2, "version": 1})
        quoted = self.ok("POST", base + "/quote", {"pickup": True})
        first = self.ok("POST", base + "/checkout", {"version": 2, "pickup": True})
        second = self.ok("POST", base + "/checkout", {"version": 2, "pickup": True})
        self.assertEqual(first["order_id"], second["order_id"])
        self.assertEqual(first["invoice"]["total_cents"], quoted["total_cents"])
        self.assertEqual(self.stock()["reserved"], 2)
        self.assertEqual(self.raw("PUT", base + "/items", {"sku": "TEA", "quantity": 1, "version": 3})["status"], 409)
