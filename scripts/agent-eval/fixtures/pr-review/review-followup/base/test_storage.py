from api import storage_status


def test_boundaries():
    for used, expected in [(0, "clear"), (890, "clear"), (895, "clear"),
                           (900, "warning"), (1000, "blocked")]:
        assert storage_status({"used": used, "capacity": 1000}) == {"status": expected}


if __name__ == "__main__":
    test_boundaries()
