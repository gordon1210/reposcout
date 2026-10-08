from shipping import endpoint


def test_quotes():
    for region, express, expected in [("domestic", False, 499), ("international", False, 999),
                                      ("domestic", True, 799), ("international", True, 1299)]:
        assert endpoint({"region": region, "express": express}) == {"shipping_fee_cents": expected}


if __name__ == "__main__":
    test_quotes()
