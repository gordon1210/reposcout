from tests.support import CommerceTest


class OutboxTests(CommerceTest):
    def test_local_retry_waits_for_due_tick(self):
        self.receive(1)
        self.order(1)
        failed = self.ok("POST", "/admin/outbox/drain", {"tick": 0, "simulate_failure": True}, "a-admin")
        self.assertEqual(failed["delivered_count"], 0)
        early = self.ok("POST", "/admin/outbox/drain", {"tick": 0}, "a-admin")
        self.assertEqual(early["delivered_count"], 0)
        due = self.ok("POST", "/admin/outbox/drain", {"tick": 1}, "a-admin")
        self.assertEqual(due["delivered_count"], 2)
        replay = self.ok("POST", "/admin/outbox/drain", {"tick": 2}, "a-admin")
        self.assertEqual(replay["delivered_count"], 0)
        self.assertEqual(self.ok("GET", "/admin/messages", token="a-admin")["outbox"], {"pending": 0, "delivered": 2, "exhausted": 0})

    def test_retry_attempts_are_bounded(self):
        self.receive(1)
        self.order(1)
        for tick in (0, 1, 3, 7, 15):
            self.ok("POST", "/admin/outbox/drain", {"tick": tick, "simulate_failure": True}, "a-admin")
        state = self.ok("GET", "/admin/messages", token="a-admin")["outbox"]
        self.assertEqual(state, {"pending": 0, "delivered": 0, "exhausted": 2})
