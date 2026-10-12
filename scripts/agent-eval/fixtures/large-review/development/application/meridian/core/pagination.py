from meridian.core.errors import require


def page(rows, request, key):
    limit = request.get("limit", 25)
    require(type(limit) is int and 1 <= limit <= 100, "invalid_limit", "limit must be 1..100")
    cursor = request.get("after")
    require(cursor is None or isinstance(cursor, str), "invalid_cursor", "after must be a string")
    ordered = sorted(rows, key=key)
    if cursor is not None:
        ordered = [row for row in ordered if key(row) > cursor]
    selected = ordered[:limit]
    next_cursor = key(selected[-1]) if len(ordered) > limit else None
    return selected, next_cursor
