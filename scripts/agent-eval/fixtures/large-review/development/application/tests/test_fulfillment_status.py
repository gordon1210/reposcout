from tests.support import CommerceTest


class FulfillmentStatusTests(CommerceTest):
    def test_partial_ship_cancel_remaining_then_return(self):
        self.receive(4)
        order = self.order(4)
        self.assertEqual(order["status"], "reserved")
        shipment = self.ship(order["order_id"], 2)
        self.assertEqual(self.show(order["order_id"])["status"], "partially_shipped")
        cancelled = self.cancel(order["order_id"], 2)
        self.assertEqual(cancelled["order"]["status"], "fulfilled")
        body = {"shipment_id": shipment["shipment_id"], "quantity": 2, "days": 4, "reason": "unwanted"}
        result = self.ok("POST", f"/orders/{order['order_id']}/returns", body)
        self.assertEqual(result["order"]["status"], "returned")

    def test_all_cancelled_status_is_distinct(self):
        self.receive(2)
        order = self.order(2)
        result = self.cancel(order["order_id"], 2)
        self.assertEqual(result["order"]["status"], "cancelled")
