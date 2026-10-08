from tests.support import CommerceTest


class AuditTests(CommerceTest):
    def test_audit_records_actor_and_scopes_tenants(self):
        self.receive(1)
        order = self.order(1)
        self.cancel(order["order_id"], 1)
        result = self.ok("GET", "/admin/audit", {"action": "order.cancel"}, "a-admin")
        self.assertEqual(len(result["entries"]), 1)
        self.assertEqual(result["entries"][0]["actor"], "alice")
        self.assertEqual(result["entries"][0]["details"], {"sku": "TEA", "quantity": 1})
        self.assertEqual(self.ok("GET", "/admin/audit", token="b-admin")["entries"], [])
