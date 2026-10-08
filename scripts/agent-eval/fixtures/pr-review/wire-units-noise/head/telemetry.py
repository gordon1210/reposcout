def event_name(route, status):
    cleaned = route.strip().replace("/", ".")
    return f"{cleaned}:{status}"


def summarize(events):
    counts = {}
    for event in events:
        name = event_name(
            event["route"],
            event["status"],
        )
        counts[name] = counts.get(name, 0) + 1
    return sorted(counts.items())
