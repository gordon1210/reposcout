from tests.support import CommerceTest


class RecurringScheduleTests(CommerceTest):
    def setup_subscription(self, schedule=None, start="2026-01-05", plan_id="regular", key="regular"):
        self.ok("POST", "/admin/recurring/plans", {"plan_id": plan_id, "name": "Regular tea",
                "items": [{"sku": "TEA", "quantity": 2}],
                "schedule": schedule or {"unit": "week", "every": 1}}, "a-admin")
        return self.ok("POST", "/subscriptions", {"plan_id": plan_id, "start_date": start, "enrollment_key": key})

    def plan(self, through, **extra):
        return self.ok("POST", "/admin/recurring/plan", dict(through=through, **extra), "a-admin")

    def test_month_end_forecast_is_anchored_and_leap_year_aware(self):
        row = self.setup_subscription({"unit": "month"}, "2028-01-31")
        forecast = self.ok("POST", f"/subscriptions/{row['subscription_id']}/forecast", {"count": 4})
        self.assertEqual([item["due_date"] for item in forecast["future_cycles"]],
                         ["2028-01-31", "2028-02-29", "2028-03-31", "2028-04-30"])
        result = self.plan("2028-03-31")
        self.assertEqual(len(result["cycles"]), 3)
        current = self.ok("GET", f"/subscriptions/{row['subscription_id']}")
        self.assertEqual(current["next_date"], "2028-04-30")

    def test_dry_run_and_small_batches_preserve_due_date_order(self):
        first = self.setup_subscription(start="2026-01-05")
        second = self.setup_subscription(start="2026-01-06", plan_id="other", key="other")
        dry = self.plan("2026-01-20", limit=3, dry_run=True)
        self.assertEqual([row["due_date"] for row in dry["cycles"]], ["2026-01-05", "2026-01-06", "2026-01-12"])
        self.assertTrue(dry["has_more"])
        self.assertEqual(self.ok("GET", f"/subscriptions/{first['subscription_id']}")["next_index"], 0)
        batch1 = self.plan("2026-01-20", limit=3)
        batch2 = self.plan("2026-01-20", limit=3)
        self.assertTrue(batch1["has_more"])
        self.assertFalse(batch2["has_more"])
        self.assertEqual([row["due_date"] for row in batch2["cycles"]], ["2026-01-13", "2026-01-19", "2026-01-20"])
        self.assertEqual(len(self.app.state.recurring_cycles), 6)

    def test_pause_and_skip_materialize_zero_charge_cycles_with_unit_accounting(self):
        row = self.setup_subscription()
        base = f"/subscriptions/{row['subscription_id']}"
        self.ok("POST", base + "/pause", {"version": 1, "pause": {"from": "2026-01-05", "through": "2026-01-12"}})
        self.ok("POST", base + "/skip", {"version": 2, "index": 2, "reason": "travel"})
        result = self.plan("2026-01-26")
        self.assertEqual([item["status"] for item in result["cycles"]], ["skipped", "skipped", "skipped", "planned"])
        statement = self.ok("GET", base + "/statement")
        self.assertEqual(statement["units"][0]["scheduled"], 8)
        self.assertEqual(statement["units"][0]["skipped"], 6)
        self.assertEqual(statement["units"][0]["pending"], 2)
        self.assertEqual(statement["billing"]["invoiced_cents"], 0)
        skipped = result["cycles"][0]["cycle_id"]
        self.assertEqual(self.raw("POST", f"/admin/recurring/cycles/{skipped}/execute",
                                 {"as_of": "2026-01-26"}, "a-admin")["status"], 409)

    def test_pause_resume_and_skip_undo_require_current_version(self):
        row = self.setup_subscription()
        base = f"/subscriptions/{row['subscription_id']}"
        pause = {"from": "2026-01-05", "through": "2026-01-12"}
        self.ok("POST", base + "/pause", {"version": 1, "pause": pause})
        self.assertEqual(self.raw("POST", base + "/resume", {"version": 1, "pause": pause})["status"], 409)
        self.ok("POST", base + "/resume", {"version": 2, "pause": pause})
        self.ok("POST", base + "/skip", {"version": 3, "index": 0})
        self.ok("POST", base + "/unskip", {"version": 4, "index": 0})
        self.assertEqual(self.plan("2026-01-05")["cycles"][0]["status"], "planned")

    def test_plan_change_only_affects_unplanned_cycles_and_preserves_consent(self):
        row = self.setup_subscription()
        base = f"/subscriptions/{row['subscription_id']}"
        old = self.plan("2026-01-05")["cycles"][0]
        self.ok("POST", "/admin/recurring/plans", {"plan_id": "large", "name": "Larger tea",
                "items": [{"sku": "TEA", "quantity": 3}], "schedule": {"unit": "week"}}, "a-admin")
        changed = self.ok("POST", base + "/change-plan", {"version": 2, "plan_id": "large", "multiplier": 2})
        self.assertEqual(changed["pending_change"]["items"][0]["quantity"], 6)
        self.assertEqual(changed["items"][0]["quantity"], 2)
        next_cycle = self.plan("2026-01-12")["cycles"][0]
        self.assertEqual(next_cycle["items"][0]["quantity"], 6)
        self.assertEqual(old["items"][0]["quantity"], 2)
        self.assertIsNone(self.ok("GET", base)["pending_change"])
        self.receive(8)
        for current in (old, next_cycle):
            self.ok("POST", f"/admin/recurring/cycles/{current['cycle_id']}/execute", {"as_of": "2026-01-12"}, "a-admin")
        statement = self.ok("GET", base + "/statement")
        self.assertEqual(statement["units"][0]["ordered"], 8)
        self.assertEqual(statement["billing"]["invoiced_cents"], 10998)

    def test_stop_voids_unexecuted_cycles_but_existing_order_remains_visible(self):
        row = self.setup_subscription()
        self.receive(2)
        cycles = self.plan("2026-01-12")["cycles"]
        placed = self.ok("POST", f"/admin/recurring/cycles/{cycles[0]['cycle_id']}/execute",
                         {"as_of": "2026-01-05"}, "a-admin")
        base = f"/subscriptions/{row['subscription_id']}"
        stopped = self.ok("POST", base + "/stop", {"version": 3, "reason": "moving"})
        self.assertEqual(stopped["status"], "stopped")
        self.assertIsNone(stopped["next_date"])
        self.assertEqual(self.plan("2026-02-01")["cycles"], [])
        statement = self.ok("GET", base + "/statement")
        self.assertEqual(statement["cycle_counts"], {"ordered": 1, "void": 1})
        self.assertEqual(self.show(placed["order"]["order_id"])["status"], "reserved")
        self.assertEqual(self.stock()["reserved"], 2)

    def test_retirement_blocks_new_enrollment_but_preserves_existing_schedule(self):
        row = self.setup_subscription()
        self.ok("POST", "/admin/recurring/plans/regular/retire", {"version": 1}, "a-admin")
        self.assertEqual(self.ok("GET", "/recurring/plans")["plans"], [])
        rejected = self.raw("POST", "/subscriptions", {"plan_id": "regular", "start_date": "2026-01-06",
                            "enrollment_key": "new"})
        self.assertEqual(rejected["status"], 409)
        self.assertEqual(len(self.plan("2026-01-05")["cycles"]), 1)

    def test_numeric_boolean_date_and_pause_validation(self):
        row = self.setup_subscription()
        base = f"/subscriptions/{row['subscription_id']}"
        for body in ({"through": "2026-01-05", "limit": True},
                     {"through": "2026-02-30"}, {"through": "2026-01-05", "dry_run": 1},
                     {"through": "2026-01-05", "limit": -1}):
            with self.subTest(body=body):
                self.assertEqual(self.raw("POST", "/admin/recurring/plan", body, "a-admin")["status"], 422)
        self.assertEqual(self.raw("POST", base + "/skip", {"version": 1, "index": True})["status"], 422)
        self.assertEqual(self.raw("POST", base + "/pause", {"version": 1,
                                 "pause": {"from": "2026-02-01", "through": "2026-01-01"}})["status"], 422)
        self.assertEqual(self.raw("PUT", base + "/checkout", {"version": 1,
                                 "checkout": {"allow_backorder": "yes"}})["status"], 422)
        self.assertEqual(self.ok("GET", base)["version"], 1)
