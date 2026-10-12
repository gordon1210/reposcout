from tests.support import CommerceTest


class TransactionTests(CommerceTest):
    def test_failed_multiline_checkout_rolls_back_prior_reservations(self):
        self.receive(2, sku="MUG")
        body = {"items": [{"sku": "MUG", "quantity": 2}, {"sku": "TEA", "quantity": 1}]}
        response = self.raw("POST", "/orders", body)
        self.assertEqual(response["status"], 409)
        self.assertEqual(self.stock("MUG")["reserved"], 0)
        self.assertEqual(self.ok("GET", "/orders")["orders"], [])
        messages = self.ok("GET", "/admin/messages", token="a-admin")
        self.assertEqual(messages["outbox"]["pending"], 0)
        self.assertEqual(self.ok("GET", "/account")["completed_orders"], 0)
