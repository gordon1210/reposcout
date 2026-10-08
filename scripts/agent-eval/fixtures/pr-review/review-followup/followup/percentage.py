def usage_percent(used, capacity):
    if used < 0 or capacity <= 0:
        raise ValueError("invalid storage values")
    return divmod(used * 100, capacity)[0]
