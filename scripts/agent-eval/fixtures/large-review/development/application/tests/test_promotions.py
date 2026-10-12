from tests.support import CommerceTest


class PromotionTests(CommerceTest):
    def test_storefront_code_and_threshold_are_exact(self):
        for code, units, discount in (("WELCOME", 1, 0), ("WELCOME", 2, 500), ("welcome", 2, 0), ("WELCOME-team", 2, 0)):
            result = self.ok("POST", "/quotes", {"items": [{"sku": "TEA", "quantity": units}], "promotion": code, "pickup": True})
            self.assertEqual(result["discount_cents"], discount)

    def test_promotion_is_not_reused_after_placing_order(self):
        self.receive(4)
        first = self.order(2, promotion="WELCOME", pickup=True)
        second = self.order(2, promotion="WELCOME", pickup=True)
        self.assertEqual(first["invoice"]["total_cents"], 2000)
        self.assertEqual(second["invoice"]["total_cents"], 2500)
