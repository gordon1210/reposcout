from tests.support import CommerceTest


class OrderTests(CommerceTest):
    def test_customer_page_has_stable_cursor(self):
        self.receive(5)
        ids = [self.order(1)["order_id"] for _ in range(3)]
        first = self.ok("GET", "/orders", {"limit": 2})
        self.assertEqual([row["order_id"] for row in first["orders"]], ids[:2])
        last = self.ok("GET", "/orders", {"limit": 2, "after": first["next_cursor"]})
        self.assertEqual([row["order_id"] for row in last["orders"]], ids[2:])

    def test_unknown_and_other_tenant_orders_are_hidden(self):
        self.receive(1)
        order = self.order(1)
        self.assertEqual(self.raw("GET", f"/orders/{order['order_id']}", token="b-customer")["status"], 404)
        self.assertEqual(self.raw("GET", "/orders/absent")["status"], 404)
