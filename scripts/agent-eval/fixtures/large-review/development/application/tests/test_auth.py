from tests.support import CommerceTest


class AuthenticationTests(CommerceTest):
    def test_credentials_and_roles_are_required(self):
        self.assertEqual(self.raw("GET", "/catalog", token="missing")["status"], 401)
        self.assertEqual(self.raw("GET", "/reports/stock")["status"], 403)
        self.assertEqual(self.raw("POST", "/events/acme", {}, "a-customer")["status"], 403)
        self.assertEqual(self.raw("GET", "/not-a-route")["status"], 404)

    def test_body_customer_id_does_not_select_identity(self):
        self.receive(1)
        created = self.order(1, customer_id="bob", tenant="a")
        self.assertEqual(len(self.ok("GET", "/orders")["orders"]), 1)
        self.assertEqual(self.ok("GET", "/orders", token="b-customer")["orders"], [])
        self.assertEqual(self.show(created["order_id"])["order_id"], created["order_id"])
