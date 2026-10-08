from tests.support import CommerceTest


class BundleInventoryTests(CommerceTest):
    def test_bundle_capacity_uses_limiting_component(self):
        self.receive(7)
        self.receive(3, sku="MUG")
        detail = self.ok("GET", "/catalog/KIT")
        self.assertEqual(detail["available"], 3)
        self.assertEqual(detail["components"], {"TEA": 1, "MUG": 1})

    def test_bundles_are_discovery_only_until_components_selected(self):
        response = self.raw("POST", "/orders", {"items": [{"sku": "KIT", "quantity": 1}]})
        self.assertEqual(response["error"]["code"], "bundle_checkout")
        self.assertEqual(self.raw("GET", "/stock/KIT", token="a-admin")["status"], 422)
