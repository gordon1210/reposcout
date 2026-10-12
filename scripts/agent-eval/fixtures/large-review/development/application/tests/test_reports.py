from tests.support import CommerceTest


class ReportingTests(CommerceTest):
    def test_stock_sales_and_customer_reports_share_order_facts(self):
        self.receive(4)
        order = self.order(3, pickup=True)
        self.ship(order["order_id"], 1)
        stock = self.ok("GET", "/reports/stock", token="a-admin")
        sales = self.ok("GET", "/reports/sales", token="a-admin")
        customers = self.ok("GET", "/reports/customers", token="a-admin")
        self.assertEqual((stock["total_on_hand"], stock["total_reserved"]), (3, 2))
        self.assertTrue(stock["reconciliation"]["consistent"])
        self.assertEqual(sales["products"], [{"sku": "TEA", "ordered": 3, "shipped": 1, "cancelled": 0, "returned": 0}])
        self.assertEqual(customers["total_due_cents"], 3750)
        self.assertEqual(self.raw("GET", "/reports/unknown", token="a-admin")["status"], 404)
