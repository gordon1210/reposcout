from api import dispatch


def test_retail_quote():
    assert dispatch("/shipping/quote", {"region": "domestic"})["shipping_fee_cents"] == 499
    assert dispatch("/shipping/quote", {"region": "international"})["shipping_fee_cents"] == 999


if __name__ == "__main__":
    test_retail_quote()
