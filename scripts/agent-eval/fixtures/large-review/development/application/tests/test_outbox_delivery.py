import unittest
from meridian.events.subscriptions import delivery_body
from meridian.events.retry import retry_at


class DeliveryPayloadTests(unittest.TestCase):
    def test_analytics_omits_payment_reference(self):
        message = {"message_id": "message-1", "type": "payment.captured", "data": {"order_id": "order-1", "payment_reference": "provider-reference", "amount_cents": 100}}
        self.assertNotIn("payment_reference", delivery_body(message, "analytics")["data"])
        self.assertIn("payment_reference", delivery_body(message, "accounting")["data"])
        self.assertIn("payment_reference", message["data"])

    def test_retry_schedule_caps_delay(self):
        self.assertEqual([retry_at(attempt, 10) for attempt in (1, 2, 3, 10)], [11, 12, 14, 74])
