from tests.support import CommerceTest


class CreditTests(CommerceTest):
    def test_full_cancellation_creates_goods_credit_and_refundable_balance(self):
        self.receive(2)
        order = self.order(2, pickup=True)
        data = {"order_id": order["order_id"], "amount_cents": 2500, "payment_reference": "paid"}
        self.assertEqual(self.event("paid-event", "payment.captured", data)["status"], 200)
        cancelled = self.cancel(order["order_id"], 2)
        invoice = cancelled["order"]["invoice"]
        self.assertEqual((invoice["credited_cents"], invoice["due_cents"], invoice["refundable_cents"]), (2500, 0, 2500))
        credits = self.ok("GET", f"/orders/{order['order_id']}/credits")["credits"]
        self.assertEqual([(row["reason"], row["amount_cents"]) for row in credits], [("cancellation", 2500)])
