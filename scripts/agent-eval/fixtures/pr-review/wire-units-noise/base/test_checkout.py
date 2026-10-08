from api import dispatch


def test_wire_units():
    assert dispatch("POST /checkout", {"items": [{"quantity": 2, "unit_price_cents": 1250}]}) == {"amount_cents": 2500, "currency": "EUR"}
    assert dispatch("POST /checkout", {"items": [{"quantity": 1, "unit_price_cents": 99}]}) == {"amount_cents": 99, "currency": "EUR"}


if __name__ == "__main__":
    test_wire_units()
