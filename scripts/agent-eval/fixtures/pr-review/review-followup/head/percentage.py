def usage_percent(used, capacity):
    if used < 0 or capacity <= 0:
        raise ValueError("invalid storage values")
    return (used * 100 + capacity - 1) // capacity
