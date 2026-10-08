"""Delivery exception resolution and courier claims, separate from customer credits."""
from meridian.core.audit import record
from meridian.core.errors import require
from meridian.events.outbox import publish
from meridian.fulfillment.tracking import record_tracking
from meridian.logistics.common import integer, lookup, public, text, tick

KINDS = {"delay", "address", "damage", "lost"}
RESOLUTIONS = {"delay": {"resumed", "delivered"}, "address": {"resumed", "delivered", "returned"},
               "damage": {"delivered", "returned", "claim"}, "lost": {"delivered", "claim"}}


def open_exception(state, context, carton_id, body):
    carton = lookup(state.logistics_cartons, context.tenant, carton_id, "carton")
    require(carton["status"] == "dispatched", "carton_state", "Only dispatched cartons can have delivery exceptions", 409)
    kind = body.get("kind")
    require(isinstance(kind, str) and kind in KINDS, "invalid_delivery_exception", "Unknown exception kind")
    require(not all(state.shipments[(context.tenant, identity)]["delivered"] for identity in carton["shipment_ids"]),
            "carton_delivered", "A delivered carton cannot acquire a transit exception", 409)
    require(not any(owner == context.tenant and row["carton_id"] == carton_id and row["status"] == "open"
                    for (owner, _), row in state.logistics_exceptions.items()),
            "exception_already_open", "Resolve the existing carton exception first", 409)
    opened = tick(body.get("tick"))
    manifest = lookup(state.logistics_manifests, context.tenant, carton["manifest_id"], "manifest")
    require(opened >= manifest["dispatch_tick"], "exception_before_dispatch", "Exception predates dispatch")
    exception_id = state.next_id(context.tenant, "delivery-exception")
    row = {"exception_id": exception_id, "carton_id": carton_id, "kind": kind,
           "description": text(body.get("description"), "description", 500), "opened_tick": opened,
           "status": "open", "resolution": None, "resolved_tick": None, "notes": [], "claim_id": None}
    state.logistics_exceptions[(context.tenant, exception_id)] = row
    publish(state, context.tenant, "logistics.delivery.exception", {"exception_id": exception_id,
            "order_id": carton["order_id"], "kind": kind})
    record(state, context, "logistics.exception.open", exception_id)
    return public(row)


def add_note(state, context, exception_id, body):
    row = lookup(state.logistics_exceptions, context.tenant, exception_id, "delivery_exception")
    require(row["status"] == "open", "exception_state", "Exception is already resolved", 409)
    at = tick(body.get("tick"))
    previous = row["notes"][-1]["tick"] if row["notes"] else row["opened_tick"]
    require(at >= previous, "exception_tick_regression", "Notes must have nondecreasing ticks")
    row["notes"].append({"tick": at, "actor": context.actor, "text": text(body.get("text"), "text", 500)})
    record(state, context, "logistics.exception.note", exception_id)
    return public(row)


def resolve_exception(state, context, exception_id, body):
    row = lookup(state.logistics_exceptions, context.tenant, exception_id, "delivery_exception")
    require(row["status"] == "open", "exception_state", "Exception is already resolved", 409)
    resolution = body.get("resolution")
    require(isinstance(resolution, str) and resolution in RESOLUTIONS[row["kind"]],
            "invalid_exception_resolution", "Resolution is not allowed for this exception")
    at = tick(body.get("tick"))
    previous = row["notes"][-1]["tick"] if row["notes"] else row["opened_tick"]
    require(at >= previous, "exception_tick_regression", "Resolution predates the latest exception evidence")
    if resolution == "claim":
        require(row["claim_id"] is not None, "claim_required", "File a courier claim before this resolution", 409)
    carton = lookup(state.logistics_cartons, context.tenant, row["carton_id"], "carton")
    if resolution == "delivered":
        location = text(body.get("location"), "location")
        for shipment_id in carton["shipment_ids"]:
            record_tracking(state, context.tenant, shipment_id, "delivered", location)
    row.update(status="resolved", resolution=resolution, resolved_tick=at)
    record(state, context, "logistics.exception.resolve", exception_id, {"resolution": resolution})
    publish(state, context.tenant, "logistics.delivery.resolved", {"exception_id": exception_id,
            "order_id": carton["order_id"], "resolution": resolution})
    return public(row)


def file_claim(state, context, exception_id, body):
    exception = lookup(state.logistics_exceptions, context.tenant, exception_id, "delivery_exception")
    require(exception["status"] == "open" and exception["kind"] in {"lost", "damage"},
            "claim_not_allowed", "Only open loss or damage exceptions support claims", 409)
    require(exception["claim_id"] is None, "claim_exists", "Exception already has a claim", 409)
    carton = lookup(state.logistics_cartons, context.tenant, exception["carton_id"], "carton")
    require(not any(owner == context.tenant and row["carton_id"] == carton["carton_id"]
                    for (owner, _), row in state.logistics_claims.items()),
            "carton_claim_exists", "Carton already has a courier claim", 409)
    amount = integer(body.get("requested_cents"), "requested_cents", 1, 100000000)
    require(amount <= carton["declared_cents"], "claim_value_exceeded", "Claim exceeds declared goods value")
    evidence = body.get("evidence")
    require(isinstance(evidence, list) and 1 <= len(evidence) <= 10,
            "claim_evidence_required", "Supply one to ten evidence references")
    evidence = [text(value, "evidence", 160) for value in evidence]
    require(len(set(evidence)) == len(evidence), "duplicate_claim_evidence", "Evidence references must be distinct")
    at = tick(body.get("tick"))
    require(at >= exception["opened_tick"], "claim_tick", "Claim predates exception")
    claim_id = state.next_id(context.tenant, "courier-claim")
    row = {"claim_id": claim_id, "exception_id": exception_id, "carton_id": carton["carton_id"],
           "manifest_id": carton["manifest_id"], "requested_cents": amount, "evidence": evidence,
           "filed_tick": at, "status": "filed", "approved_cents": 0, "paid_cents": 0,
           "decision_tick": None, "decision_reference": None, "payments": []}
    state.logistics_claims[(context.tenant, claim_id)] = row
    exception["claim_id"] = claim_id
    record(state, context, "logistics.claim.file", claim_id, {"requested_cents": amount})
    return public(row)


def decide_claim(state, context, claim_id, body):
    claim = lookup(state.logistics_claims, context.tenant, claim_id, "courier_claim")
    require(claim["status"] == "filed", "claim_state", "Claim has already been decided", 409)
    amount = integer(body.get("approved_cents"), "approved_cents", 0, 100000000)
    require(amount <= claim["requested_cents"], "claim_approval_exceeded", "Approval exceeds requested amount")
    at = tick(body.get("tick"))
    require(at >= claim["filed_tick"], "claim_tick", "Decision predates claim")
    reference = text(body.get("reference"), "reference")
    claim.update(status="approved" if amount else "rejected", approved_cents=amount,
                 decision_tick=at, decision_reference=reference)
    record(state, context, "logistics.claim.decide", claim_id, {"approved_cents": amount})
    return public(claim)


def recover_claim(state, context, claim_id, body):
    claim = lookup(state.logistics_claims, context.tenant, claim_id, "courier_claim")
    require(claim["status"] in {"approved", "part_paid"}, "claim_state", "Claim is not payable", 409)
    amount = integer(body.get("amount_cents"), "amount_cents", 1, 100000000)
    require(claim["paid_cents"] + amount <= claim["approved_cents"],
            "claim_overpayment", "Recovery exceeds approved claim amount")
    reference = text(body.get("reference"), "reference")
    require(not any(owner == context.tenant and any(payment["reference"] == reference for payment in row["payments"])
                    for (owner, _), row in state.logistics_claims.items()),
            "duplicate_claim_payment", "Recovery reference has already been recorded", 409)
    at = tick(body.get("tick"))
    previous = claim["payments"][-1]["tick"] if claim["payments"] else claim["decision_tick"]
    require(at >= previous, "claim_tick", "Recovery predates the latest claim entry")
    claim["payments"].append({"reference": reference, "amount_cents": amount, "tick": at})
    claim["paid_cents"] += amount
    claim["status"] = "paid" if claim["paid_cents"] == claim["approved_cents"] else "part_paid"
    record(state, context, "logistics.claim.recover", claim_id, {"amount_cents": amount})
    return public(claim)
