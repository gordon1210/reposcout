from tests.support import CommerceTest


class CatalogSearchTests(CommerceTest):
    def test_search_by_tag_and_category(self):
        result = self.ok("GET", "/catalog", {"query": "BREAKFAST", "category": "grocery"})
        self.assertEqual([row["sku"] for row in result["products"]], ["TEA"])
        self.assertIsNone(result["next_cursor"])

    def test_visibility_and_cursor(self):
        first = self.ok("GET", "/catalog", {"limit": 1})
        self.assertEqual([row["sku"] for row in first["products"]], ["KIT"])
        second = self.ok("GET", "/catalog", {"limit": 1, "after": first["next_cursor"]})
        self.assertEqual([row["sku"] for row in second["products"]], ["MUG"])
        self.assertEqual(self.raw("GET", "/catalog/CASE")["status"], 404)
