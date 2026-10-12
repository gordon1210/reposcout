from policy import quota_status


def storage_status(request):
    return {"status": quota_status(request["used"], request["capacity"])}
