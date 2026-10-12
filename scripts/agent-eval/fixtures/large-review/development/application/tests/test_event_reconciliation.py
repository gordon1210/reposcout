from tests.support import CommerceTest


class EventReconciliationTests(CommerceTest):
    def manifest(self, events, tenant="a", provider="acme"):
        return self.ok("POST", f"/admin/events/{provider}/reconcile", {"events": events}, f"{tenant}-admin")

    def test_reconciliation_reports_missing_conflicting_and_unlisted_receipts(self):
        data = {"warehouse": "north", "sku": "TEA", "quantity": 3}
        self.event("received", "stock.received", data)
        self.event("unlisted", "stock.received", data)
        report = self.manifest([{"event_id": "received", "type": "stock.received"},
                                {"event_id": "pending", "type": "payment.captured"}])
        self.assertEqual(report["matched_count"], 1)
        self.assertEqual(report["applied"][0]["outcome"]["on_hand"], 3)
        self.assertEqual(report["missing"], ["pending"])
        self.assertEqual(report["unlisted"], ["unlisted"])
        self.assertFalse(report["complete"])
        mismatch = self.manifest([{"event_id": "received", "type": "payment.captured"}])
        self.assertEqual(mismatch["type_conflicts"], [{"event_id": "received", "declared_type": "payment.captured", "applied_type": "stock.received"}])
        self.assertEqual(self.stock()["on_hand"], 6)

    def test_reconciliation_is_scoped_to_authenticated_tenant_and_provider(self):
        data = {"warehouse": "north", "sku": "TEA", "quantity": 2}
        self.event("north-a", "stock.received", data)
        self.event("north-b", "stock.received", data, tenant="b")
        self.event("other-provider", "stock.received", data, provider="backup")
        a = self.manifest([{"event_id": "north-a", "type": "stock.received"}])
        b = self.manifest([{"event_id": "north-b", "type": "stock.received"}], tenant="b")
        self.assertTrue(a["complete"])
        self.assertTrue(b["complete"])
        self.assertEqual(a["unlisted"], [])
        self.assertEqual(b["unlisted"], [])
        forbidden = self.raw("POST", "/admin/events/acme/reconcile", {"events": []})
        self.assertEqual(forbidden["status"], 403)

    def test_manifest_rejects_duplicates_without_mutating_receipts(self):
        entry = {"event_id": "duplicate", "type": "stock.received"}
        response = self.raw("POST", "/admin/events/acme/reconcile", {"events": [entry, entry]}, "a-admin")
        self.assertEqual(response["error"]["code"], "duplicate_manifest_event")
        self.assertEqual(self.ok("GET", "/admin/messages", token="a-admin")["inbox"]["applied_count"], 0)
