from tests.test_purchasing_receiving import PurchasingSetup


class PurchasingReplenishmentTests(PurchasingSetup):
    def rule(self, **options):
        return self.ok("PUT", "/purchasing/replenishment/rules/TEA", dict(supplier_id="leaves", warehouse="north", reorder_point=3, target_quantity=10, **options), "a-admin")

    def test_planning_rounds_packs_and_accounts_for_shortages(self):
        self.supplier(pack_size=4)
        self.rule()
        self.order(5, allow_backorder=True)
        preview = self.ok("GET", "/purchasing/replenishment", token="a-admin")
        self.assertEqual(preview["proposals"][0]["quantity"], 16)
        self.assertEqual(preview["proposals"][0]["backorder_quantity"], 5)
        self.assertEqual(preview["total_cents"], 4800)
        plan = self.ok("POST", "/purchasing/replenishment/plans", token="a-admin")
        converted = self.ok("POST", f"/purchasing/replenishment/plans/{plan['plan_id']}/convert", {"expected_version": 1}, "a-admin")
        self.assertEqual(len(converted["purchase_order_ids"]), 1)
        again = self.ok("POST", f"/purchasing/replenishment/plans/{plan['plan_id']}/convert", {"expected_version": 1}, "a-admin")
        self.assertEqual(converted, again)

    def test_stock_change_invalidates_saved_proposal(self):
        self.supplier()
        self.rule()
        plan = self.ok("POST", "/purchasing/replenishment/plans", token="a-admin")
        self.receive(5)
        response = self.raw("POST", f"/purchasing/replenishment/plans/{plan['plan_id']}/convert", {"expected_version": 1}, "a-admin")
        self.assertEqual(response["status"], 409)

    def test_approved_inbound_prevents_duplicate_replenishment(self):
        self.supplier()
        self.rule()
        self.purchase(10)
        self.assertEqual(self.ok("GET", "/purchasing/replenishment", token="a-admin")["proposals"], [])

    def test_boolean_quantities_and_invalid_switches_rejected(self):
        self.supplier()
        response = self.raw("PUT", "/purchasing/replenishment/rules/TEA", {"supplier_id": "leaves", "warehouse": "north", "reorder_point": True, "target_quantity": 10}, "a-admin")
        self.assertEqual(response["status"], 422)
        response = self.raw("POST", "/purchasing/orders", {"supplier_id": "leaves", "warehouse": "north", "lines": [{"sku": "TEA", "quantity": True}]}, "a-admin")
        self.assertEqual(response["status"], 422)
