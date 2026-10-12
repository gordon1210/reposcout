"""Supplier qualification and independently versioned product purchasing terms."""
from meridian.core.errors import require
from meridian.core.identity import identifier
from meridian.core.audit import record
from meridian.catalog.repository import get_product
from meridian.purchasing.validation import integer, text, boolean, lookup, snapshot, version


def supplier(state, tenant, supplier_id, active=False):
    row = lookup(state.purchase_suppliers, tenant, supplier_id, "supplier")
    require(not active or row["status"] == "active", "supplier_unavailable", "Supplier is not active", 409)
    return row


def onboard(state, context, body):
    supplier_id = identifier(body.get("supplier_id"), "supplier_id")
    require((context.tenant, supplier_id) not in state.purchase_suppliers,
            "supplier_exists", "Supplier identifier already exists", 409)
    row = {"supplier_id": supplier_id, "tenant": context.tenant,
           "name": text(body.get("name"), "name"),
           "contact": text(body.get("contact"), "contact"),
           "currency": "EUR", "status": "pending", "version": 1,
           "payment_days": integer(body.get("payment_days", 30), "payment_days", 0, 180),
           "qualification_reference": None, "suspension_reason": None}
    state.purchase_suppliers[(context.tenant, supplier_id)] = row
    record(state, context, "supplier.onboarded", supplier_id)
    return snapshot(row)


def qualify(state, context, supplier_id, body):
    row = supplier(state, context.tenant, supplier_id)
    version(row, body.get("expected_version"))
    require(row["status"] in {"pending", "suspended"}, "supplier_status", "Supplier is already active", 409)
    reference = text(body.get("qualification_reference"), "qualification_reference")
    row.update(status="active", qualification_reference=reference, suspension_reason=None,
               version=row["version"] + 1)
    record(state, context, "supplier.qualified", supplier_id, {"reference": reference})
    return snapshot(row)


def suspend(state, context, supplier_id, body):
    row = supplier(state, context.tenant, supplier_id)
    version(row, body.get("expected_version"))
    require(row["status"] == "active", "supplier_status", "Only active suppliers may be suspended", 409)
    reason = text(body.get("reason"), "reason")
    row.update(status="suspended", suspension_reason=reason, version=row["version"] + 1)
    record(state, context, "supplier.suspended", supplier_id, {"reason": reason})
    return snapshot(row)


def put_terms(state, context, supplier_id, sku, body):
    supplier(state, context.tenant, supplier_id, active=True)
    sku = identifier(sku, "sku")
    product = get_product(state, context.tenant, sku)
    require(not product.components, "bundle_purchase", "Purchase component SKUs rather than bundles")
    key = (context.tenant, supplier_id, sku)
    old = state.purchase_terms.get(key)
    if old:
        version(old, body.get("expected_version"))
    else:
        require("expected_version" not in body, "stale_version", "Terms do not yet exist", 409)
    pack = integer(body.get("pack_size", 1), "pack_size", 1, 1_000_000)
    minimum = integer(body.get("minimum_quantity", pack), "minimum_quantity", 1, 1_000_000)
    require(minimum % pack == 0, "invalid_pack_quantity", "Minimum must be a pack multiple")
    row = {"tenant": context.tenant, "supplier_id": supplier_id, "sku": sku,
           "unit_cents": integer(body.get("unit_cents"), "unit_cents", 1),
           "pack_size": pack, "minimum_quantity": minimum,
           "lead_days": integer(body.get("lead_days", 7), "lead_days", 0, 365),
           "active": boolean(body.get("active", True), "active"),
           "supplier_sku": text(body.get("supplier_sku", sku), "supplier_sku", 80),
           "version": old["version"] + 1 if old else 1}
    state.purchase_terms[key] = row
    record(state, context, "supplier.terms_changed", supplier_id, {"sku": sku, "version": row["version"]})
    return snapshot(row)


def terms_for(state, tenant, supplier_id, sku):
    supplier(state, tenant, supplier_id, active=True)
    get_product(state, tenant, sku)
    row = state.purchase_terms.get((tenant, supplier_id, sku))
    require(row is not None and row["active"], "terms_unavailable", "No active product terms", 409)
    return row


def supplier_view(state, tenant, supplier_id):
    result = snapshot(supplier(state, tenant, supplier_id))
    result["terms"] = [snapshot(row) for (owner, vendor, _), row in sorted(state.purchase_terms.items())
                       if owner == tenant and vendor == supplier_id]
    return result
