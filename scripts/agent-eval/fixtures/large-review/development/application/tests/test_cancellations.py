from tests.support import CommerceTest


class CancellationTests(CommerceTest):
    def test_cancel_remaining_units_preserves_shipment(self):
        self.receive(5)
        order = self.order(5)
        shipment = self.ship(order["order_id"], 2)
        result = self.cancel(order["order_id"], 3)
        self.assertEqual(result["cancelled_quantity"], 3)
        self.assertEqual(result["order"]["lines"][0], {"sku": "TEA", "ordered": 5, "shipped": 2, "cancelled": 3, "returned": 0, "reserved": 0, "unallocated": 0})
        self.assertEqual(self.stock()["available"], 3)
        self.assertEqual(shipment["quantity"], 2)

    def test_zero_is_noop_and_invalid_requests_are_atomic(self):
        self.receive(2)
        order = self.order(2)
        zero = self.cancel(order["order_id"], 0)
        self.assertEqual(zero["order"], order)
        for count, status in ((3, 409), (-1, 422), (True, 422)):
            response = self.raw("POST", f"/orders/{order['order_id']}/cancel", {"sku": "TEA", "quantity": count})
            self.assertEqual(response["status"], status)
        self.assertEqual(self.show(order["order_id"]), order)
