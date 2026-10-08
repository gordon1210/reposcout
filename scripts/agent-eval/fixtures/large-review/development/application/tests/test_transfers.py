from tests.support import CommerceTest


class TransferTests(CommerceTest):
    def test_transfer_preserves_totals_and_cannot_move_reservations(self):
        self.receive(8)
        self.order(5)
        body = {"sku": "TEA", "source": "north", "destination": "south", "quantity": 4}
        self.assertEqual(self.raw("POST", "/stock/transfer", body, "a-warehouse")["status"], 409)
        body["quantity"] = 3
        self.ok("POST", "/stock/transfer", body, "a-warehouse")
        result = self.stock()
        self.assertEqual((result["on_hand"], result["reserved"], result["available"]), (8, 5, 3))
        self.assertEqual([row["on_hand"] for row in result["warehouses"]], [5, 3])
