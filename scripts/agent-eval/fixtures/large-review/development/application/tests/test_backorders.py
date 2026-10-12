from tests.support import CommerceTest


class BackorderTests(CommerceTest):
    def test_backorder_can_be_allocated_after_receipt(self):
        order = self.order(3, allow_backorder=True)
        self.assertEqual(order["status"], "awaiting_stock")
        self.assertEqual(order["lines"][0]["unallocated"], 3)
        self.receive(2, warehouse="south")
        allocated = self.ok("POST", f"/warehouse/orders/{order['order_id']}/allocate", token="a-warehouse")
        self.assertEqual(allocated["lines"][0]["reserved"], 2)
        self.assertEqual(allocated["lines"][0]["unallocated"], 1)
        report = self.ok("GET", "/reports/fulfillment", token="a-admin")
        self.assertEqual(report["backorders"], [{"order_id": order["order_id"], "missing": {"TEA": 1}}])
