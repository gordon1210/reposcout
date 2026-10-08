from api import dispatch


def test_last_refund_day():
    for path in ("/refund", "/support/refund"):
        assert dispatch(path, {"days": 14}) == {"allowed": True}


if __name__ == "__main__":
    test_last_refund_day()
