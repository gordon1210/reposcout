from tests.support import CommerceTest


class ProviderEventTests(CommerceTest):
    def test_replay_applies_once_and_providers_have_distinct_ids(self):
        data = {"warehouse": "north", "sku": "TEA", "quantity": 3}
        first = self.event("receipt", "stock.received", data)
        replay = self.event("receipt", "stock.received", data)
        alternate = self.event("receipt", "stock.received", data, provider="backup")
        self.assertTrue(first["data"]["applied"])
        self.assertTrue(replay["data"]["duplicate"])
        self.assertTrue(alternate["data"]["applied"])
        self.assertEqual(self.stock()["on_hand"], 6)

    def test_trusted_context_ignores_body_tenant_and_allows_only_bound_providers(self):
        data = {"warehouse": "north", "sku": "TEA", "quantity": 2}
        result = self.event("body-tenant", "stock.received", data, tenant="b", tenant_id="tenant-a")
        self.assertEqual(result["status"], 200)
        self.assertEqual(self.stock(tenant="b")["on_hand"], 2)
        self.assertEqual(self.stock()["on_hand"], 0)
        self.assertEqual(self.event("unbound", "stock.received", data, provider="unknown")["status"], 403)
