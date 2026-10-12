from api import dispatch


def test_delivery_pass():
    assert dispatch("POST /shipping/quote", {"delivery_pass": True}) == {"shipping_fee_cents": 0}
    assert dispatch("POST /shipping/quote", {"delivery_pass": False}) == {"shipping_fee_cents": 499}


if __name__ == "__main__":
    test_delivery_pass()
