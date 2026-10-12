"""Logistics HTTP adapters; dispatcher supplies authorization and atomicity."""
from meridian.logistics import carriers, exceptions, manifests, packing, projections, rates, waves
from meridian.logistics.common import boolean, lookup, public


def profile(app, context, body, params):
    return packing.set_profile(app.state, context, params["sku"], body)


def services(app, context, body, params):
    return carriers.service_listing(app.state, context.tenant,
                                   boolean(body.get("include_inactive", False), "include_inactive"))


def register_service(app, context, body, params):
    return carriers.register_service(app.state, context, body)


def retire_service(app, context, body, params):
    return carriers.retire_service(app.state, context, params["service_id"])


def preview_wave(app, context, body, params):
    return waves.plan_wave(app.state, app.settings, context.tenant, body)


def create_wave(app, context, body, params):
    return waves.create_wave(app.state, app.settings, context, body)


def show_wave(app, context, body, params):
    return waves.wave_view(app.state, context.tenant, params["wave_id"])


def start_wave(app, context, body, params):
    return waves.start_wave(app.state, context, params["wave_id"], body)


def pick(app, context, body, params):
    return waves.pick_line(app.state, context, params["wave_id"], body)


def cancel_wave(app, context, body, params):
    return waves.cancel_wave(app.state, context, params["wave_id"])


def carton(app, context, body, params):
    return packing.create_carton(app.state, context, params["wave_id"], body)


def show_carton(app, context, body, params):
    return public(lookup(app.state.logistics_cartons, context.tenant, params["carton_id"], "carton"))


def seal(app, context, body, params):
    return packing.seal_carton(app.state, context, params["carton_id"], body)


def void(app, context, body, params):
    return packing.void_carton(app.state, context, params["carton_id"])


def finish_pack(app, context, body, params):
    return packing.complete_packing(app.state, context, params["wave_id"])


def compare(app, context, body, params):
    return rates.compare_rates(app.state, context.tenant, params["carton_id"], body)


def create_manifest(app, context, body, params):
    return manifests.create_manifest(app.state, app.settings, context, body)


def show_manifest(app, context, body, params):
    return manifests.manifest_document(app.state, context.tenant, params["manifest_id"])


def add_carton(app, context, body, params):
    return manifests.add_carton(app.state, context, params["manifest_id"], body)


def remove_carton(app, context, body, params):
    return manifests.remove_carton(app.state, context, params["manifest_id"], params["carton_id"])


def close_manifest(app, context, body, params):
    return manifests.close_manifest(app.state, context, params["manifest_id"])


def dispatch_manifest(app, context, body, params):
    return manifests.dispatch_manifest(app.state, app.settings, context, params["manifest_id"], body)


def exception(app, context, body, params):
    return exceptions.open_exception(app.state, context, params["carton_id"], body)


def exception_note(app, context, body, params):
    return exceptions.add_note(app.state, context, params["exception_id"], body)


def resolve_exception(app, context, body, params):
    return exceptions.resolve_exception(app.state, context, params["exception_id"], body)


def file_claim(app, context, body, params):
    return exceptions.file_claim(app.state, context, params["exception_id"], body)


def decide_claim(app, context, body, params):
    return exceptions.decide_claim(app.state, context, params["claim_id"], body)


def recover_claim(app, context, body, params):
    return exceptions.recover_claim(app.state, context, params["claim_id"], body)


def customer_shipments(app, context, body, params):
    return projections.customer_shipments(app.state, context, params["order_id"])


def summary(app, context, body, params):
    return projections.operations_summary(app.state, context.tenant, body)
