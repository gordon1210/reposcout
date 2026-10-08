"""Operations purchasing endpoints use the credential's tenant exclusively."""
from meridian.purchasing import suppliers, orders, receiving, matching, credits, replenishment, reconciliation
from meridian.purchasing.validation import lookup, snapshot


def supplier_create(app, context, body, params):
    return suppliers.onboard(app.state, context, body)


def supplier_show(app, context, body, params):
    return suppliers.supplier_view(app.state, context.tenant, params["supplier_id"])


def supplier_qualify(app, context, body, params):
    return suppliers.qualify(app.state, context, params["supplier_id"], body)


def supplier_suspend(app, context, body, params):
    return suppliers.suspend(app.state, context, params["supplier_id"], body)


def supplier_terms(app, context, body, params):
    return suppliers.put_terms(app.state, context, params["supplier_id"], params["sku"], body)


def supplier_statement(app, context, body, params):
    return reconciliation.supplier_statement(app.state, context.tenant, params["supplier_id"])


def purchase_create(app, context, body, params):
    return orders.create_draft(app.state, app.settings, context, body)


def purchase_show(app, context, body, params):
    return orders.order_view(orders.purchase_order(app.state, context.tenant, params["purchase_order_id"]))


def purchase_amend(app, context, body, params):
    return orders.amend_draft(app.state, app.settings, context, params["purchase_order_id"], body)


def purchase_approve(app, context, body, params):
    return orders.approve(app.state, context, params["purchase_order_id"], body)


def purchase_close(app, context, body, params):
    return orders.close(app.state, context, params["purchase_order_id"], body)


def purchase_receive(app, context, body, params):
    return receiving.receive(app.state, app.settings, context, params["purchase_order_id"], body)


def receipt_show(app, context, body, params):
    return receiving.receipt_view(lookup(app.state.purchase_receipts, context.tenant, params["receipt_id"], "receipt"))


def receipt_inspect(app, context, body, params):
    return receiving.inspect(app.state, app.settings, context, params["receipt_id"], body)


def purchase_reconcile(app, context, body, params):
    return reconciliation.reconcile_purchase(app.state, context.tenant, params["purchase_order_id"])


def invoice_create(app, context, body, params):
    return matching.create_invoice(app.state, context, params["purchase_order_id"], body)


def invoice_show(app, context, body, params):
    return matching.invoice_view(app.state, lookup(app.state.purchase_invoices, context.tenant, params["invoice_id"], "vendor_invoice"))


def invoice_match(app, context, body, params):
    return matching.match_invoice(app.state, context, params["invoice_id"], body)


def invoice_correct(app, context, body, params):
    return matching.correct_invoice(app.state, context, params["invoice_id"], body)


def invoice_settle(app, context, body, params):
    return matching.settle(app.state, context, params["invoice_id"], body)


def credit_create(app, context, body, params):
    return credits.issue_credit(app.state, context, params["receipt_id"], body)


def credit_apply(app, context, body, params):
    return credits.apply_credit(app.state, context, params["credit_id"], body)


def replenishment_rule(app, context, body, params):
    return replenishment.set_rule(app.state, app.settings, context, params["sku"], body)


def replenishment_preview(app, context, body, params):
    return replenishment.propose(app.state, app.settings, context.tenant, body.get("warehouse"))


def replenishment_plan(app, context, body, params):
    return replenishment.save_plan(app.state, app.settings, context, body)


def replenishment_show(app, context, body, params):
    return snapshot(lookup(app.state.replenishment_plans, context.tenant, params["plan_id"], "replenishment_plan"))


def replenishment_convert(app, context, body, params):
    return replenishment.convert_plan(app.state, app.settings, context, params["plan_id"], body)
