from percentage import usage_percent


def quota_status(used, capacity):
    percent = usage_percent(used, capacity)
    if percent >= 100:
        return "blocked"
    if percent >= 90:
        return "warning"
    return "clear"
