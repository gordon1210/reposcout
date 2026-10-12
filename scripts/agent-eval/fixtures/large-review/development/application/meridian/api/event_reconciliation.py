from meridian.events.reconciliation import reconcile_provider_receipts


def reconcile(app, context, body, params):
    return reconcile_provider_receipts(app.state, context.tenant, params["provider"], body)
