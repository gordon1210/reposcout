from meridian.recurring import plans, enrollment, changes, planning, renewals, projections, repository


def catalog(app, context, body, params):
    return {"plans": [plans.plan_view(row) for row in repository.plans(app.state, context.tenant)]}


def plan_create(app, context, body, params):
    return plans.plan_view(plans.create_plan(app.state, context, body))


def plan_revise(app, context, body, params):
    return plans.plan_view(plans.revise_plan(app.state, context, params["plan_id"], body))


def plan_retire(app, context, body, params):
    return plans.plan_view(plans.retire_plan(app.state, context, params["plan_id"], body))


def enroll(app, context, body, params):
    return projections.subscription_view(enrollment.enroll(app, context, body))


def listing(app, context, body, params):
    return projections.customer_listing(app.state, context, body)


def show(app, context, body, params):
    row = repository.subscription(app.state, context.tenant, params["subscription_id"], context.actor)
    return projections.subscription_view(row)


def statement(app, context, body, params):
    row = repository.subscription(app.state, context.tenant, params["subscription_id"], context.actor)
    return projections.statement(app.state, context, row)


def audit(app, context, body, params):
    row = repository.subscription(app.state, context.tenant, params["subscription_id"], context.actor)
    return projections.audit_trail(app.state, context, row)


def forecast(app, context, body, params):
    return planning.forecast(app.state, context, params["subscription_id"], body)


def pause(app, context, body, params):
    return projections.subscription_view(changes.pause(app.state, context, params["subscription_id"], body))


def resume(app, context, body, params):
    return projections.subscription_view(changes.remove_pause(app.state, context, params["subscription_id"], body))


def skip(app, context, body, params):
    return projections.subscription_view(changes.skip(app.state, context, params["subscription_id"], body))


def unskip(app, context, body, params):
    return projections.subscription_view(changes.unskip(app.state, context, params["subscription_id"], body))


def change_plan(app, context, body, params):
    return projections.subscription_view(changes.change_plan(app, context, params["subscription_id"], body))


def discard_change(app, context, body, params):
    return projections.subscription_view(changes.discard_change(app.state, context, params["subscription_id"], body))


def change_checkout(app, context, body, params):
    return projections.subscription_view(changes.change_checkout(app, context, params["subscription_id"], body))


def stop(app, context, body, params):
    return projections.subscription_view(changes.stop(app.state, context, params["subscription_id"], body))


def plan_due(app, context, body, params):
    return planning.plan_due(app.state, context, body)


def preview(app, context, body, params):
    return renewals.preview(app, context, params["cycle_id"], body)


def execute(app, context, body, params):
    return renewals.execute(app, context, params["cycle_id"], body)


def hold(app, context, body, params):
    return projections.cycle_view(app.state, renewals.hold(app.state, context, params["cycle_id"], body))


def release(app, context, body, params):
    return projections.cycle_view(app.state, renewals.release(app.state, context, params["cycle_id"], body))


def cycle_show(app, context, body, params):
    return projections.cycle_view(app.state, repository.cycle(app.state, context.tenant, params["cycle_id"], context.actor))


def report(app, context, body, params):
    return projections.operational_report(app.state, context, body)
