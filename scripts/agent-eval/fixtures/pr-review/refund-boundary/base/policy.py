def eligible(days):
    if days < 0:
        raise ValueError("negative age")
    return days <= 14
