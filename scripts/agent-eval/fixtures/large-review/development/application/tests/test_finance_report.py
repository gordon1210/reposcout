from tests.support import CommerceTest


class FinanceReportTests(CommerceTest):
    def test_report_accounts_for_returns_and_payment(self):
        self.receive(2)
        order = self.order(2, pickup=True)
        shipment = self.ship(order["order_id"], 2)
        self.event("settled", "payment.captured", {"order_id": order["order_id"], "amount_cents": 2500, "payment_reference": "settlement"})
        self.ok("POST", f"/orders/{order['order_id']}/returns", {"shipment_id": shipment["shipment_id"], "quantity": 1, "days": 1, "reason": "wrong_item"})
        report = self.ok("GET", "/reports/finance", token="a-admin")
        self.assertEqual((report["invoiced_cents"], report["captured_cents"], report["credited_cents"], report["due_cents"]), (2500, 2500, 1250, 0))
        self.assertTrue(report["reconciliation"]["consistent"])
