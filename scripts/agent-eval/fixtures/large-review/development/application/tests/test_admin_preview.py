from tests.support import CommerceTest


class AdminPreviewTests(CommerceTest):
    def test_preview_has_independent_staff_policy(self):
        result = self.ok("POST", "/admin/promotion-preview", {"code": "WELCOME", "subtotal_cents": 200}, "a-admin")
        self.assertEqual(result, {"preview": True, "eligible": True, "discount_cents": 200, "estimated_cents": 0})
        quote = self.ok("POST", "/quotes", {"items": [{"sku": "TEA", "quantity": 1}], "promotion": "WELCOME", "pickup": True})
        self.assertEqual(quote["discount_cents"], 0)
        self.assertEqual(self.raw("POST", "/admin/promotion-preview", {"code": "WELCOME", "subtotal_cents": 200})["status"], 403)
