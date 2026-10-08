from tests.support import CommerceTest


class OrderNoteTests(CommerceTest):
    def test_internal_notes_are_hidden_from_customer(self):
        self.receive(1)
        order = self.order(1)
        order_id = order["order_id"]
        self.ok("POST", f"/orders/{order_id}/notes", {"text": " Leave at reception. "})
        self.ok("POST", f"/admin/orders/{order_id}/notes", {"text": "Check packing quality"}, "a-admin")
        notes = self.show(order_id)["notes"]
        self.assertEqual([note["text"] for note in notes], ["Leave at reception."])
        self.assertEqual(self.raw("POST", f"/orders/{order_id}/notes", {"text": " "})["status"], 422)
