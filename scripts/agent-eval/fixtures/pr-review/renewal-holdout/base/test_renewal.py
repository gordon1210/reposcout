from service import dispatch


def test_last_hour():
    for kind in ("manual", "scheduled"):
        assert dispatch(kind, {"hours": 24}) == {"renewed": True}


if __name__ == "__main__":
    test_last_hour()
