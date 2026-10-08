from api import dispatch


def test_storefront_contract():
    for code, subtotal, expected in [("WELCOME", 1999, 0), ("WELCOME", 2000, 500),
                                     ("WELCOME-X", 2000, 0)]:
        assert dispatch("/storefront/discount", {"code": code, "subtotal_cents": subtotal}) == {"discount_cents": expected}


if __name__ == "__main__":
    test_storefront_contract()
