from tests.support import CommerceTest


class EventValidationTests(CommerceTest):
    def test_rejected_event_can_be_corrected_and_retried(self):
        data = {"warehouse": "north", "sku": "TEA", "quantity": 0}
        self.assertEqual(self.event("retryable", "stock.received", data)["status"], 422)
        self.assertEqual(self.stock()["on_hand"], 0)
        data["quantity"] = 4
        result = self.event("retryable", "stock.received", data)
        self.assertTrue(result["data"]["applied"])
        self.assertEqual(self.stock()["on_hand"], 4)
        receipt = self.ok("GET", "/admin/messages", token="a-admin")["inbox"]
        self.assertEqual(receipt["applied_count"], 1)

    def test_unknown_event_and_missing_data_reject(self):
        self.assertEqual(self.event("unknown", "inventory.magic", {})["status"], 422)
        response = self.raw("POST", "/events/acme", {"event_id": "empty", "type": "stock.received"}, "a-provider")
        self.assertEqual(response["status"], 422)
