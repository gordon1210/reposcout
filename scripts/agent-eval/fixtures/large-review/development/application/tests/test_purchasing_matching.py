from tests.test_purchasing_receiving import PurchasingSetup


class PurchasingMatchingTests(PurchasingSetup):
    def invoice(self, purchase, units, cents=300, reference="vendor-1"):
        return self.ok("POST", f"/purchasing/orders/{purchase['purchase_order_id']}/invoices", {"vendor_reference": reference, "lines": [{"sku": "TEA", "quantity": units, "unit_cents": cents}]}, "a-admin")

    def match(self, invoice):
        return self.ok("POST", f"/purchasing/invoices/{invoice['invoice_id']}/match", {"expected_version": invoice["version"]}, "a-admin")

    def test_match_excludes_quarantine_and_price_variance(self):
        self.supplier()
        purchase = self.purchase(10)
        self.receipt(purchase, 4, 6)
        invoice = self.match(self.invoice(purchase, 5, 310))
        self.assertEqual(invoice["status"], "disputed")
        self.assertEqual({row["kind"] for row in invoice["match"]["issues"]}, {"price", "quantity"})
        corrected = self.ok("PUT", f"/purchasing/invoices/{invoice['invoice_id']}", {"expected_version": 2, "reason": "bill inspected goods only", "lines": [{"sku": "TEA", "quantity": 4, "unit_cents": 300}]}, "a-admin")
        self.assertEqual(self.match(corrected)["status"], "matched")

    def test_two_invoices_cannot_consume_same_receipt_units(self):
        self.supplier()
        purchase = self.purchase(5)
        self.receipt(purchase, 5)
        first = self.match(self.invoice(purchase, 4))
        second = self.match(self.invoice(purchase, 2, reference="vendor-2"))
        self.assertEqual(first["status"], "matched")
        self.assertEqual(second["status"], "disputed")
        self.assertEqual(second["match"]["issues"][0]["accepted_available"], 1)

    def test_rejection_credit_and_settlement(self):
        self.supplier()
        purchase = self.purchase(10)
        receipt = self.receipt(purchase, 8, rejected=2)
        invoice = self.match(self.invoice(purchase, 8))
        credit_body = {"vendor_reference": "credit-1", "reason": "damaged packaging", "lines": [{"sku": "TEA", "quantity": 2}]}
        credit = self.ok("POST", f"/purchasing/receipts/{receipt['receipt_id']}/credits", credit_body, "a-admin")
        replay = self.ok("POST", f"/purchasing/receipts/{receipt['receipt_id']}/credits", credit_body, "a-admin")
        self.assertEqual(credit, replay)
        self.assertEqual(credit["amount_cents"], 600)
        self.ok("POST", f"/purchasing/credits/{credit['credit_id']}/apply", {"invoice_id": invoice["invoice_id"]}, "a-admin")
        settled = self.ok("POST", f"/purchasing/invoices/{invoice['invoice_id']}/settle", {"expected_version": 3, "settlement_reference": "bank-transfer-1", "amount_cents": 1800}, "a-admin")
        self.assertEqual(settled["status"], "settled")
        self.assertEqual(self.stock()["on_hand"], 8)
        statement = self.ok("GET", "/purchasing/suppliers/leaves/statement", token="a-admin")
        self.assertEqual((statement["payable_cents"], statement["settled_cents"], statement["unapplied_credit_cents"]), (0, 1800, 0))
        response = self.raw("POST", f"/purchasing/receipts/{receipt['receipt_id']}/credits", dict(credit_body, vendor_reference="credit-2"), "a-admin")
        self.assertEqual(response["status"], 409)
