from tests.support import CommerceTest


class ReturnTests(CommerceTest):
    def test_partial_return_restock_and_credit(self):
        self.receive(3)
        order = self.order(3, pickup=True)
        shipment = self.ship(order["order_id"], 3)
        result = self.ok("POST", f"/orders/{order['order_id']}/returns", {
            "shipment_id": shipment["shipment_id"], "quantity": 1, "days": 30, "reason": "unwanted"})
        self.assertEqual(result["credit_cents"], 1250)
        self.assertTrue(result["restocked"])
        self.assertEqual(self.stock()["on_hand"], 1)
        self.assertEqual(result["order"]["status"], "partially_returned")

    def test_damaged_returns_do_not_reenter_saleable_stock(self):
        self.receive(1)
        order = self.order(1)
        shipment = self.ship(order["order_id"], 1)
        body = {"shipment_id": shipment["shipment_id"], "quantity": 1, "days": 31, "reason": "damaged"}
        path = f"/orders/{order['order_id']}/returns"
        self.assertEqual(self.raw("POST", path, body)["status"], 422)
        body["days"] = 0
        self.assertFalse(self.ok("POST", path, body)["restocked"])
        self.assertEqual(self.stock()["available"], 0)
        self.assertEqual(self.raw("POST", path, body)["status"], 409)
