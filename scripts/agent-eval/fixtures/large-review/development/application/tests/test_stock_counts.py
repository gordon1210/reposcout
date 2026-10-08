from tests.support import CommerceTest


class StockCountTests(CommerceTest):
    def test_count_cannot_remove_reserved_units(self):
        self.receive(5)
        self.order(3)
        bad = self.raw("POST", "/stock/count", {"sku": "TEA", "warehouse": "north", "counted": 2, "reason": "count"}, "a-warehouse")
        self.assertEqual(bad["status"], 409)
        self.assertEqual(self.stock()["on_hand"], 5)
        counted = self.ok("POST", "/stock/count", {"sku": "TEA", "warehouse": "north", "counted": 4, "reason": "count"}, "a-warehouse")
        self.assertEqual(counted["available"], 1)
