from tests.support import CommerceTest


class RecurringOrdersTests(CommerceTest):
    def create_plan(self, plan_id="weekly-tea", units=2, **extra):
        return self.ok("POST", "/admin/recurring/plans",
                       dict(plan_id=plan_id, name="Weekly tea", items=[{"sku": "TEA", "quantity": units}],
                            schedule={"unit": "week", "every": 1}, **extra), "a-admin")

    def enroll(self, **extra):
        return self.ok("POST", "/subscriptions",
                       dict(plan_id="weekly-tea", start_date="2026-01-05", enrollment_key="home-tea", **extra))

    def plan_cycles(self, through="2026-01-05", **extra):
        return self.ok("POST", "/admin/recurring/plan", dict(through=through, **extra), "a-admin")["cycles"]

    def renew(self, cycle_id, as_of="2026-01-05", **extra):
        return self.ok("POST", f"/admin/recurring/cycles/{cycle_id}/execute", dict(as_of=as_of, **extra), "a-admin")

    def test_due_cycle_creates_customer_order_once_and_reserves_real_stock(self):
        self.create_plan()
        self.receive(5)
        enrolled = self.enroll()
        cycles = self.plan_cycles()
        renewed = self.renew(cycles[0]["cycle_id"], customer_id="bob", tenant="tenant-b")
        self.assertEqual(renewed["order"]["invoice"]["total_cents"], 2999)
        self.assertEqual(self.stock()["reserved"], 2)
        visible = self.show(renewed["order"]["order_id"])
        self.assertEqual(visible["lines"][0]["ordered"], 2)
        replay = self.renew(cycles[0]["cycle_id"])
        self.assertTrue(replay["replayed"])
        self.assertEqual(replay["order"]["order_id"], renewed["order"]["order_id"])
        self.assertEqual(self.stock()["reserved"], 2)
        self.assertEqual(self.plan_cycles(), [])
        statement = self.ok("GET", f"/subscriptions/{enrolled['subscription_id']}/statement")
        self.assertEqual(statement["billing"]["due_cents"], 2999)
        self.assertEqual(statement["units"][0]["ordered"], 2)

    def test_enrollment_retry_does_not_duplicate_or_adopt_new_plan_revision(self):
        self.create_plan()
        first = self.enroll()
        self.ok("PUT", "/admin/recurring/plans/weekly-tea",
                {"version": 1, "items": [{"sku": "TEA", "quantity": 3}]}, "a-admin")
        second = self.enroll()
        self.assertEqual(first, second)
        conflict = self.raw("POST", "/subscriptions", {"plan_id": "weekly-tea", "start_date": "2026-01-05",
                            "enrollment_key": "home-tea", "multiplier": 2})
        self.assertEqual(conflict["status"], 409)
        listing = self.ok("GET", "/subscriptions")
        self.assertEqual(len(listing["subscriptions"]), 1)

    def test_stock_failure_rolls_back_then_retries_after_supplied_date(self):
        self.create_plan()
        self.enroll()
        cycle_id = self.plan_cycles()[0]["cycle_id"]
        failed = self.renew(cycle_id)
        self.assertEqual(failed["cycle"]["status"], "retry_wait")
        self.assertEqual(failed["cycle"]["next_attempt_date"], "2026-01-07")
        self.assertEqual(self.ok("GET", "/orders")["orders"], [])
        self.assertEqual(self.stock()["reserved"], 0)
        self.receive(2)
        early = self.raw("POST", f"/admin/recurring/cycles/{cycle_id}/execute", {"as_of": "2026-01-06"}, "a-admin")
        self.assertEqual(early["status"], 409)
        success = self.renew(cycle_id, "2026-01-07")
        self.assertEqual(success["cycle"]["status"], "ordered")
        self.assertEqual(len(success["cycle"]["attempts"]), 2)
        self.assertEqual(success["order"]["order_id"], "order-0001")
        self.assertEqual(self.stock()["reserved"], 2)

    def test_exhaustion_requires_explicit_additional_attempt_and_keeps_history(self):
        self.create_plan(maximum_attempts=1)
        self.enroll()
        cycle_id = self.plan_cycles()[0]["cycle_id"]
        failed = self.renew(cycle_id)
        self.assertEqual(failed["cycle"]["status"], "held")
        release_path = f"/admin/recurring/cycles/{cycle_id}/release"
        blocked = self.raw("POST", release_path, {"version": 2, "as_of": "2026-01-06"}, "a-admin")
        self.assertEqual(blocked["status"], 409)
        released = self.ok("POST", release_path,
                           {"version": 2, "as_of": "2026-01-06", "additional_attempts": 1}, "a-admin")
        self.assertEqual(released["maximum_attempts"], 2)
        self.receive(2)
        succeeded = self.renew(cycle_id, "2026-01-06")
        self.assertEqual(len(succeeded["cycle"]["attempts"]), 2)

    def test_multiline_stock_failure_leaves_no_first_line_reservation(self):
        self.ok("POST", "/admin/recurring/plans", {"plan_id": "breakfast", "name": "Breakfast",
                "items": [{"sku": "MUG", "quantity": 1}, {"sku": "TEA", "quantity": 2}],
                "schedule": {"unit": "week"}}, "a-admin")
        self.receive(1, sku="MUG")
        self.ok("POST", "/subscriptions", {"plan_id": "breakfast", "enrollment_key": "breakfast",
                "start_date": "2026-01-05"})
        result = self.renew(self.plan_cycles()[0]["cycle_id"])
        self.assertEqual(result["cycle"]["status"], "retry_wait")
        self.assertEqual(self.stock("MUG")["reserved"], 0)
        self.assertEqual(self.ok("GET", "/orders")["orders"], [])
        self.assertEqual(len(self.app.state.invoices), 0)

    def test_backorder_consent_yields_real_awaiting_stock_order(self):
        self.create_plan()
        self.enroll(checkout={"allow_backorder": True, "pickup": True})
        result = self.renew(self.plan_cycles()[0]["cycle_id"])
        self.assertEqual(result["cycle"]["status"], "ordered")
        self.assertEqual(result["order"]["status"], "awaiting_stock")
        self.assertEqual(result["order"]["invoice"]["total_cents"], 2500)

    def test_customer_and_tenant_ownership_are_not_request_fields(self):
        self.create_plan()
        enrolled = self.enroll(customer_id="bob", tenant="tenant-b")
        base = f"/subscriptions/{enrolled['subscription_id']}"
        self.assertEqual(self.raw("GET", base, token="b-customer")["status"], 404)
        self.assertEqual(self.raw("POST", "/admin/recurring/plan", {"through": "2026-01-05"})["status"], 403)
        self.assertEqual(self.ok("GET", "/subscriptions", token="b-customer")["subscriptions"], [])
        cycle_id = self.plan_cycles()[0]["cycle_id"]
        self.assertEqual(self.raw("GET", f"/recurring/cycles/{cycle_id}", token="b-customer")["status"], 404)
        self.assertEqual(self.raw("POST", f"/admin/recurring/cycles/{cycle_id}/execute",
                                 {"as_of": "2026-01-05"}, "b-admin")["status"], 404)

    def test_price_is_current_at_renewal_and_preview_does_not_reserve(self):
        self.create_plan()
        self.receive(4)
        self.enroll(checkout={"pickup": True})
        cycle_id = self.plan_cycles()[0]["cycle_id"]
        self.ok("PUT", "/admin/products/TEA/price", {"unit_cents": 1400}, "a-admin")
        preview = self.ok("POST", f"/admin/recurring/cycles/{cycle_id}/preview", {}, "a-admin")
        self.assertEqual(preview["quote"]["total_cents"], 2800)
        self.assertEqual(self.stock()["reserved"], 0)
        result = self.renew(cycle_id)
        self.assertEqual(result["order"]["invoice"]["total_cents"], 2800)

    def test_statement_tracks_real_payment_without_new_recurring_charge(self):
        self.create_plan()
        self.receive(2)
        enrolled = self.enroll(checkout={"pickup": True})
        order = self.renew(self.plan_cycles()[0]["cycle_id"])["order"]
        result = self.event("recurring-payment-1", "payment.captured",
                            {"order_id": order["order_id"], "amount_cents": 2500, "payment_reference": "renewal-pay-1"})
        self.assertEqual(result["status"], 200)
        statement = self.ok("GET", f"/subscriptions/{enrolled['subscription_id']}/statement")
        self.assertEqual(statement["billing"], {"currency": "EUR", "invoiced_cents": 2500,
                                               "captured_cents": 2500, "credited_cents": 0, "due_cents": 0})
        audit = self.ok("GET", f"/subscriptions/{enrolled['subscription_id']}/audit")
        self.assertIn("recurring.renewal.ordered", [entry["action"] for entry in audit["entries"]])
