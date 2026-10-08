from authorization import can_renew as permitted


def manual(request):
    return {"renewed": permitted(request["hours"])}


def scheduled(request):
    return {"renewed": permitted(request["hours"])}


JOBS = {"manual": manual, "scheduled": scheduled}


def dispatch(kind, request):
    return JOBS[kind](request)
