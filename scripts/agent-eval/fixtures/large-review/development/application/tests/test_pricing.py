from tests.support import CommerceTest


class PricingTests(CommerceTest):
    def test_tax_is_added_to_taxable_goods_only(self):
        result = self.ok("POST", "/quotes", {"items": [{"sku": "MUG", "quantity": 1}, {"sku": "TEA", "quantity": 1}]})
        self.assertEqual(result["subtotal_cents"], 2150)
        self.assertEqual(result["tax_cents"], 171)
        self.assertEqual(result["total_cents"], 2820)

    def test_duplicate_items_merge_and_bad_quantities_reject(self):
        result = self.ok("POST", "/quotes", {"items": [{"sku": "TEA", "quantity": 1}, {"sku": "TEA", "quantity": 2}], "pickup": True})
        self.assertEqual(len(result["lines"]), 1)
        self.assertEqual(result["total_cents"], 3750)
        for invalid in (0, -1, True, "2"):
            response = self.raw("POST", "/quotes", {"items": [{"sku": "TEA", "quantity": invalid}]})
            self.assertEqual(response["status"], 422)
