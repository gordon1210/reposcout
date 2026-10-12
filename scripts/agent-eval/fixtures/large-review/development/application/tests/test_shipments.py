from tests.support import CommerceTest


class ShipmentTests(CommerceTest):
    def test_shipment_consumes_stock_and_reservations_together(self):
        self.receive(5)
        order = self.order(4)
        shipped = self.ship(order["order_id"], 2)
        self.assertEqual(shipped["allocations"], [{"warehouse": "north", "quantity": 2}])
        self.assertEqual(self.show(order["order_id"])["lines"][0]["shipped"], 2)
        self.assertEqual((self.stock()["on_hand"], self.stock()["reserved"]), (3, 2))
        bad = self.raw("POST", f"/warehouse/orders/{order['order_id']}/ship", {"sku": "TEA", "quantity": 3}, "a-warehouse")
        self.assertEqual(bad["status"], 409)
        self.assertEqual(self.stock()["on_hand"], 3)
