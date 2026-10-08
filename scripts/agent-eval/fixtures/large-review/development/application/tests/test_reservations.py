from tests.support import CommerceTest


class ReservationTests(CommerceTest):
    def test_priority_allocation_splits_without_overbooking(self):
        self.receive(3)
        self.receive(5, warehouse="south")
        order = self.order(6)
        self.assertEqual([(row["warehouse"], row["reserved"]) for row in order["reservations"]], [("north", 3), ("south", 3)])
        self.assertEqual(self.stock()["available"], 2)
        repeat = self.ok("POST", f"/warehouse/orders/{order['order_id']}/allocate", token="a-warehouse")
        self.assertEqual(repeat["reservations"], order["reservations"])
        picklist = self.ok("GET", "/warehouse/south/picklist", token="a-warehouse")
        self.assertEqual(picklist["orders"][0]["items"], [{"sku": "TEA", "quantity": 3}])
