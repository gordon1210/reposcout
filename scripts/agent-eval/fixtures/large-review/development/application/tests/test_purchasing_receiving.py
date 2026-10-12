from tests.support import CommerceTest


class PurchasingSetup(CommerceTest):
    def supplier(self, tenant="a", unit_cents=300, pack_size=1):
        token = f"{tenant}-admin"
        self.ok("POST", "/purchasing/suppliers", {"supplier_id": "leaves", "name": "Leaves Cooperative", "contact": "buyer@example.test"}, token)
        self.ok("POST", "/purchasing/suppliers/leaves/qualify", {"expected_version": 1, "qualification_reference": "audit-2025"}, token)
        return self.ok("PUT", "/purchasing/suppliers/leaves/terms/TEA", {"unit_cents": unit_cents, "pack_size": pack_size}, token)

    def purchase(self, units=10):
        draft = self.ok("POST", "/purchasing/orders", {"supplier_id": "leaves", "warehouse": "north", "lines": [{"sku": "TEA", "quantity": units}]}, "a-admin")
        return self.ok("POST", f"/purchasing/orders/{draft['purchase_order_id']}/approve", {"expected_version": 1, "approval_reference": "budget-1", "authorized_cents": 100000}, "a-admin")

    def receipt(self, purchase, accepted, quarantined=0, rejected=0, **options):
        return self.ok("POST", f"/purchasing/orders/{purchase['purchase_order_id']}/receive", {"expected_version": purchase["version"], "delivery_reference": "delivery-1", "lines": [{"sku": "TEA", "accepted": accepted, "quarantined": quarantined, "rejected": rejected, "reason": "inspection required"}], **options}, "a-warehouse")


class PurchasingReceivingTests(PurchasingSetup):
    def test_partial_receipt_quarantine_and_inspection_share_inventory(self):
        self.supplier()
        purchase = self.purchase()
        receipt = self.receipt(purchase, 4, 3, 1)
        self.assertEqual(self.stock()["on_hand"], 4)
        self.assertEqual(receipt["quarantine_quantity"], 3)
        inspected = self.ok("POST", f"/purchasing/receipts/{receipt['receipt_id']}/inspect", {"expected_version": 1, "reason": "two pass laboratory check", "lines": [{"sku": "TEA", "accepted": 2, "rejected": 1}]}, "a-warehouse")
        self.assertEqual(inspected["quarantine_quantity"], 0)
        self.assertEqual(self.stock()["on_hand"], 6)
        reconciliation = self.ok("GET", f"/purchasing/orders/{purchase['purchase_order_id']}/reconcile", token="a-admin")
        self.assertTrue(reconciliation["balanced"])
        line = reconciliation["lines"][0]
        self.assertEqual((line["received"], line["accepted"], line["rejected"], line["ledger_received"]), (8, 6, 2, 6))
        self.assertEqual(reconciliation["purchase_order"]["lines"]["TEA"]["remaining"], 2)

    def test_receipt_can_allocate_real_backorders(self):
        self.supplier()
        order = self.order(5, allow_backorder=True)
        receipt = self.receipt(self.purchase(5), 5, allocate_backorders=True)
        self.assertEqual(receipt["allocations"], [{"order_id": order["order_id"], "quantity": 5}])
        self.assertEqual(self.stock()["reserved"], 5)
        self.assertEqual(self.stock()["available"], 0)

    def test_delivery_reference_replay_does_not_duplicate_stock(self):
        self.supplier()
        purchase = self.purchase(5)
        first = self.receipt(purchase, 5)
        second = self.receipt(purchase, 5)
        self.assertEqual(first, second)
        self.assertEqual(self.stock()["on_hand"], 5)
        response = self.raw("POST", f"/purchasing/orders/{purchase['purchase_order_id']}/receive", {"expected_version": 2, "delivery_reference": "delivery-1", "lines": [{"sku": "TEA", "accepted": 4}]}, "a-warehouse")
        self.assertEqual(response["status"], 409)

    def test_overreceipt_is_atomic(self):
        self.supplier()
        purchase = self.purchase(2)
        response = self.raw("POST", f"/purchasing/orders/{purchase['purchase_order_id']}/receive", {"expected_version": 2, "delivery_reference": "too-much", "lines": [{"sku": "TEA", "accepted": 3}]}, "a-warehouse")
        self.assertEqual(response["status"], 409)
        self.assertEqual(self.stock()["on_hand"], 0)

    def test_tenant_scope_and_role_boundary(self):
        self.supplier()
        purchase = self.purchase()
        path = f"/purchasing/orders/{purchase['purchase_order_id']}"
        self.assertEqual(self.raw("GET", path, token="b-admin")["status"], 404)
        self.assertEqual(self.raw("GET", path)["status"], 403)
        self.assertEqual(self.raw("POST", "/purchasing/suppliers", {"supplier_id": "bad", "name": "Bad", "contact": "x", "payment_days": True}, "a-admin")["status"], 422)

    def test_changed_terms_block_approval_but_not_approved_receipt(self):
        self.supplier()
        draft = self.ok("POST", "/purchasing/orders", {"supplier_id": "leaves", "warehouse": "north", "lines": [{"sku": "TEA", "quantity": 2}]}, "a-admin")
        self.ok("PUT", "/purchasing/suppliers/leaves/terms/TEA", {"expected_version": 1, "unit_cents": 400}, "a-admin")
        response = self.raw("POST", f"/purchasing/orders/{draft['purchase_order_id']}/approve", {"expected_version": 1, "approval_reference": "approval", "authorized_cents": 10000}, "a-admin")
        self.assertEqual(response["status"], 409)
        purchase = self.purchase(2)
        self.ok("PUT", "/purchasing/suppliers/leaves/terms/TEA", {"expected_version": 2, "unit_cents": 700}, "a-admin")
        self.assertEqual(self.receipt(purchase, 2)["accepted_cents"], 800)
