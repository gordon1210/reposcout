import unittest
from meridian.core.money import ratio_round, split_cents


class MoneyTests(unittest.TestCase):
    def test_largest_remainder_allocation_conserves_cents(self):
        self.assertEqual(split_cents(5, [1, 1, 1]), [2, 2, 1])
        self.assertEqual(split_cents(500, [1250, 900]), [291, 209])
        self.assertEqual(sum(split_cents(7, [0, 3, 2])), 7)

    def test_tax_rounding_uses_half_up(self):
        self.assertEqual(ratio_round(5, 1, 2), 3)
        self.assertEqual(ratio_round(900, 1900, 10000), 171)
