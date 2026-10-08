import unittest
from meridian.pricing.shipping import shipping_fee
from meridian.core.errors import DomainError


class ShippingTests(unittest.TestCase):
    def test_delivery_tiers(self):
        self.assertEqual(shipping_fee("domestic", 9999), 499)
        self.assertEqual(shipping_fee("domestic", 10000), 0)
        self.assertEqual(shipping_fee("international", 10000, express=True), 1999)
        self.assertEqual(shipping_fee("domestic", 10000, express=True), 700)
        self.assertEqual(shipping_fee("international", 100, pickup=True), 0)

    def test_conflicting_options_reject(self):
        with self.assertRaises(DomainError):
            shipping_fee("domestic", 1000, express=True, pickup=True)
