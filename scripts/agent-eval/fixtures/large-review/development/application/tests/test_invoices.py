from tests.support import CommerceTest


class InvoiceTests(CommerceTest):
    def test_partial_capture_and_reference_replay(self):
        self.receive(2)
        order = self.order(2, pickup=True)
        data = {"order_id": order["order_id"], "amount_cents": 1000, "payment_reference": "payment-one"}
        self.assertEqual(self.event("capture-one", "payment.captured", data)["status"], 200)
        self.assertEqual(self.event("capture-again", "payment.captured", data)["status"], 200)
        invoice = self.show(order["order_id"])["invoice"]
        self.assertEqual((invoice["captured_cents"], invoice["due_cents"]), (1000, 1500))
        data = dict(data, amount_cents=1501, payment_reference="payment-two")
        self.assertEqual(self.event("overpayment", "payment.captured", data)["status"], 409)

    def test_reference_cannot_be_reused_with_different_amount(self):
        self.receive(1)
        order = self.order(1, pickup=True)
        data = {"order_id": order["order_id"], "amount_cents": 500, "payment_reference": "same-reference"}
        self.event("charge-a", "payment.captured", data)
        data["amount_cents"] = 600
        self.assertEqual(self.event("charge-b", "payment.captured", data)["status"], 409)
