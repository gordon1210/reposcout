from tests.support import CommerceTest


class StatementTests(CommerceTest):
    def test_statement_aggregates_customer_invoices(self):
        self.receive(3)
        self.order(1, pickup=True)
        self.order(2, pickup=True)
        statement = self.ok("GET", "/account/statement")
        self.assertEqual(statement["due_cents"], 3750)
        self.assertEqual(len(statement["invoices"]), 2)
        other = self.ok("GET", "/account/statement", token="b-customer")
        self.assertEqual((other["due_cents"], other["invoices"]), (0, []))
