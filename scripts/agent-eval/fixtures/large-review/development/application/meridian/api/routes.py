import re
from dataclasses import dataclass
from meridian.api import customer_accounts, customer_baskets, customer_catalog, customer_orders
from meridian.api import customer_returns, warehouse, provider_events, admin, admin_preview, reports
from meridian.api import purchasing_api, logistics_api, recurring_api, event_reconciliation
from meridian.core.errors import DomainError


@dataclass(frozen=True)
class Route:
    method: str
    pattern: str
    roles: frozenset
    handler: object

    def match(self, method, path):
        if self.method != method:
            return None
        match = re.fullmatch(self.pattern, path)
        return match.groupdict() if match else None


CUSTOMER = frozenset({"customer"})
OPERATIONS = frozenset({"warehouse", "admin"})
ADMIN = frozenset({"admin"})
PROVIDER = frozenset({"provider"})
ROUTES = (
    Route("GET", r"/catalog", CUSTOMER, customer_catalog.listing),
    Route("GET", r"/catalog/(?P<sku>[^/]+)", CUSTOMER, customer_catalog.detail),
    Route("POST", r"/quotes", CUSTOMER, customer_catalog.price),
    Route("GET", r"/account", CUSTOMER, customer_accounts.profile),
    Route("PUT", r"/account/address", CUSTOMER, customer_accounts.address),
    Route("PUT", r"/account/preferences", CUSTOMER, customer_accounts.preferences),
    Route("GET", r"/account/statement", CUSTOMER, customer_accounts.statement),
    Route("POST", r"/baskets", CUSTOMER, customer_baskets.create),
    Route("GET", r"/baskets/(?P<basket_id>[^/]+)", CUSTOMER, customer_baskets.show),
    Route("PUT", r"/baskets/(?P<basket_id>[^/]+)/items", CUSTOMER, customer_baskets.update_item),
    Route("POST", r"/baskets/(?P<basket_id>[^/]+)/quote", CUSTOMER, customer_baskets.quote),
    Route("POST", r"/baskets/(?P<basket_id>[^/]+)/checkout", CUSTOMER, customer_baskets.checkout),
    Route("POST", r"/orders", CUSTOMER, customer_orders.place),
    Route("GET", r"/orders", CUSTOMER, customer_orders.listing),
    Route("GET", r"/orders/(?P<order_id>[^/]+)", CUSTOMER, customer_orders.show),
    Route("POST", r"/orders/(?P<order_id>[^/]+)/cancel", CUSTOMER, customer_orders.cancel),
    Route("POST", r"/orders/(?P<order_id>[^/]+)/notes", CUSTOMER, customer_orders.note),
    Route("POST", r"/orders/(?P<order_id>[^/]+)/returns", CUSTOMER, customer_returns.create),
    Route("GET", r"/orders/(?P<order_id>[^/]+)/credits", CUSTOMER, customer_returns.credits),
    Route("GET", r"/stock/(?P<sku>[^/]+)", OPERATIONS, warehouse.stock),
    Route("POST", r"/stock/receive", OPERATIONS, warehouse.receive),
    Route("POST", r"/stock/count", OPERATIONS, warehouse.count),
    Route("POST", r"/stock/transfer", OPERATIONS, warehouse.transfer),
    Route("POST", r"/warehouse/orders/(?P<order_id>[^/]+)/allocate", OPERATIONS, warehouse.allocate),
    Route("POST", r"/warehouse/orders/(?P<order_id>[^/]+)/ship", OPERATIONS, warehouse.ship),
    Route("GET", r"/warehouse/(?P<warehouse>[^/]+)/picklist", OPERATIONS, warehouse.picklist),
    Route("GET", r"/inventory/ledger", OPERATIONS, warehouse.ledger),
    Route("POST", r"/events/(?P<provider>[^/]+)", PROVIDER, provider_events.ingest),
    Route("POST", r"/admin/products", ADMIN, admin.product_create),
    Route("POST", r"/admin/products/(?P<sku>[^/]+)/retire", ADMIN, admin.product_retire),
    Route("PUT", r"/admin/products/(?P<sku>[^/]+)/price", ADMIN, admin.product_reprice),
    Route("POST", r"/admin/customers", ADMIN, admin.customer_create),
    Route("POST", r"/admin/orders/(?P<order_id>[^/]+)/notes", ADMIN, admin.internal_note),
    Route("POST", r"/admin/outbox/drain", ADMIN, admin.drain),
    Route("GET", r"/admin/messages", ADMIN, admin.messages),
    Route("POST", r"/admin/events/(?P<provider>[^/]+)/reconcile", ADMIN, event_reconciliation.reconcile),
    Route("GET", r"/admin/audit", ADMIN, admin.audit),
    Route("POST", r"/admin/promotion-preview", ADMIN, admin_preview.promotion_preview),
    Route("GET", r"/reports/(?P<report>[^/]+)", ADMIN, reports.show),
    Route('POST', r'/purchasing/suppliers', ADMIN, purchasing_api.supplier_create),
    Route('GET', r'/purchasing/suppliers/(?P<supplier_id>[^/]+)', OPERATIONS, purchasing_api.supplier_show),
    Route('POST', r'/purchasing/suppliers/(?P<supplier_id>[^/]+)/qualify', ADMIN, purchasing_api.supplier_qualify),
    Route('POST', r'/purchasing/suppliers/(?P<supplier_id>[^/]+)/suspend', ADMIN, purchasing_api.supplier_suspend),
    Route('PUT', r'/purchasing/suppliers/(?P<supplier_id>[^/]+)/terms/(?P<sku>[^/]+)', ADMIN, purchasing_api.supplier_terms),
    Route('GET', r'/purchasing/suppliers/(?P<supplier_id>[^/]+)/statement', ADMIN, purchasing_api.supplier_statement),
    Route('POST', r'/purchasing/orders', ADMIN, purchasing_api.purchase_create),
    Route('GET', r'/purchasing/orders/(?P<purchase_order_id>[^/]+)', OPERATIONS, purchasing_api.purchase_show),
    Route('PUT', r'/purchasing/orders/(?P<purchase_order_id>[^/]+)', ADMIN, purchasing_api.purchase_amend),
    Route('POST', r'/purchasing/orders/(?P<purchase_order_id>[^/]+)/approve', ADMIN, purchasing_api.purchase_approve),
    Route('POST', r'/purchasing/orders/(?P<purchase_order_id>[^/]+)/close', ADMIN, purchasing_api.purchase_close),
    Route('POST', r'/purchasing/orders/(?P<purchase_order_id>[^/]+)/receive', OPERATIONS, purchasing_api.purchase_receive),
    Route('GET', r'/purchasing/orders/(?P<purchase_order_id>[^/]+)/reconcile', OPERATIONS, purchasing_api.purchase_reconcile),
    Route('GET', r'/purchasing/receipts/(?P<receipt_id>[^/]+)', OPERATIONS, purchasing_api.receipt_show),
    Route('POST', r'/purchasing/receipts/(?P<receipt_id>[^/]+)/inspect', OPERATIONS, purchasing_api.receipt_inspect),
    Route('POST', r'/purchasing/orders/(?P<purchase_order_id>[^/]+)/invoices', ADMIN, purchasing_api.invoice_create),
    Route('GET', r'/purchasing/invoices/(?P<invoice_id>[^/]+)', ADMIN, purchasing_api.invoice_show),
    Route('PUT', r'/purchasing/invoices/(?P<invoice_id>[^/]+)', ADMIN, purchasing_api.invoice_correct),
    Route('POST', r'/purchasing/invoices/(?P<invoice_id>[^/]+)/match', ADMIN, purchasing_api.invoice_match),
    Route('POST', r'/purchasing/invoices/(?P<invoice_id>[^/]+)/settle', ADMIN, purchasing_api.invoice_settle),
    Route('POST', r'/purchasing/receipts/(?P<receipt_id>[^/]+)/credits', ADMIN, purchasing_api.credit_create),
    Route('POST', r'/purchasing/credits/(?P<credit_id>[^/]+)/apply', ADMIN, purchasing_api.credit_apply),
    Route('PUT', r'/purchasing/replenishment/rules/(?P<sku>[^/]+)', ADMIN, purchasing_api.replenishment_rule),
    Route('GET', r'/purchasing/replenishment', ADMIN, purchasing_api.replenishment_preview),
    Route('POST', r'/purchasing/replenishment/plans', ADMIN, purchasing_api.replenishment_plan),
    Route('GET', r'/purchasing/replenishment/plans/(?P<plan_id>[^/]+)', ADMIN, purchasing_api.replenishment_show),
    Route('POST', r'/purchasing/replenishment/plans/(?P<plan_id>[^/]+)/convert', ADMIN, purchasing_api.replenishment_convert),
    Route('PUT', r'/logistics/profiles/(?P<sku>[^/]+)', ADMIN, logistics_api.profile),
    Route('GET', r'/logistics/services', OPERATIONS, logistics_api.services),
    Route('POST', r'/logistics/services', ADMIN, logistics_api.register_service),
    Route('POST', r'/logistics/services/(?P<service_id>[^/]+)/retire', ADMIN, logistics_api.retire_service),
    Route('POST', r'/logistics/waves/preview', OPERATIONS, logistics_api.preview_wave),
    Route('POST', r'/logistics/waves', OPERATIONS, logistics_api.create_wave),
    Route('GET', r'/logistics/waves/(?P<wave_id>[^/]+)', OPERATIONS, logistics_api.show_wave),
    Route('POST', r'/logistics/waves/(?P<wave_id>[^/]+)/start', OPERATIONS, logistics_api.start_wave),
    Route('POST', r'/logistics/waves/(?P<wave_id>[^/]+)/pick', OPERATIONS, logistics_api.pick),
    Route('POST', r'/logistics/waves/(?P<wave_id>[^/]+)/cancel', OPERATIONS, logistics_api.cancel_wave),
    Route('POST', r'/logistics/waves/(?P<wave_id>[^/]+)/cartons', OPERATIONS, logistics_api.carton),
    Route('POST', r'/logistics/waves/(?P<wave_id>[^/]+)/pack', OPERATIONS, logistics_api.finish_pack),
    Route('GET', r'/logistics/cartons/(?P<carton_id>[^/]+)', OPERATIONS, logistics_api.show_carton),
    Route('POST', r'/logistics/cartons/(?P<carton_id>[^/]+)/seal', OPERATIONS, logistics_api.seal),
    Route('POST', r'/logistics/cartons/(?P<carton_id>[^/]+)/void', OPERATIONS, logistics_api.void),
    Route('POST', r'/logistics/cartons/(?P<carton_id>[^/]+)/rates', OPERATIONS, logistics_api.compare),
    Route('POST', r'/logistics/manifests', OPERATIONS, logistics_api.create_manifest),
    Route('GET', r'/logistics/manifests/(?P<manifest_id>[^/]+)', OPERATIONS, logistics_api.show_manifest),
    Route('POST', r'/logistics/manifests/(?P<manifest_id>[^/]+)/cartons', OPERATIONS, logistics_api.add_carton),
    Route('DELETE', r'/logistics/manifests/(?P<manifest_id>[^/]+)/cartons/(?P<carton_id>[^/]+)', OPERATIONS, logistics_api.remove_carton),
    Route('POST', r'/logistics/manifests/(?P<manifest_id>[^/]+)/close', OPERATIONS, logistics_api.close_manifest),
    Route('POST', r'/logistics/manifests/(?P<manifest_id>[^/]+)/dispatch', OPERATIONS, logistics_api.dispatch_manifest),
    Route('POST', r'/logistics/cartons/(?P<carton_id>[^/]+)/exceptions', OPERATIONS, logistics_api.exception),
    Route('POST', r'/logistics/exceptions/(?P<exception_id>[^/]+)/notes', OPERATIONS, logistics_api.exception_note),
    Route('POST', r'/logistics/exceptions/(?P<exception_id>[^/]+)/resolve', OPERATIONS, logistics_api.resolve_exception),
    Route('POST', r'/logistics/exceptions/(?P<exception_id>[^/]+)/claims', OPERATIONS, logistics_api.file_claim),
    Route('POST', r'/logistics/claims/(?P<claim_id>[^/]+)/decision', ADMIN, logistics_api.decide_claim),
    Route('POST', r'/logistics/claims/(?P<claim_id>[^/]+)/recover', ADMIN, logistics_api.recover_claim),
    Route('GET', r'/orders/(?P<order_id>[^/]+)/shipments', CUSTOMER, logistics_api.customer_shipments),
    Route('GET', r'/logistics/summary', ADMIN, logistics_api.summary),
    Route('GET', r'/recurring/plans', CUSTOMER, recurring_api.catalog),
    Route('POST', r'/admin/recurring/plans', ADMIN, recurring_api.plan_create),
    Route('PUT', r'/admin/recurring/plans/(?P<plan_id>[^/]+)', ADMIN, recurring_api.plan_revise),
    Route('POST', r'/admin/recurring/plans/(?P<plan_id>[^/]+)/retire', ADMIN, recurring_api.plan_retire),
    Route('POST', r'/subscriptions', CUSTOMER, recurring_api.enroll),
    Route('GET', r'/subscriptions', CUSTOMER, recurring_api.listing),
    Route('GET', r'/subscriptions/(?P<subscription_id>[^/]+)', CUSTOMER, recurring_api.show),
    Route('GET', r'/subscriptions/(?P<subscription_id>[^/]+)/statement', CUSTOMER, recurring_api.statement),
    Route('GET', r'/subscriptions/(?P<subscription_id>[^/]+)/audit', CUSTOMER, recurring_api.audit),
    Route('POST', r'/subscriptions/(?P<subscription_id>[^/]+)/forecast', CUSTOMER, recurring_api.forecast),
    Route('POST', r'/subscriptions/(?P<subscription_id>[^/]+)/pause', CUSTOMER, recurring_api.pause),
    Route('POST', r'/subscriptions/(?P<subscription_id>[^/]+)/resume', CUSTOMER, recurring_api.resume),
    Route('POST', r'/subscriptions/(?P<subscription_id>[^/]+)/skip', CUSTOMER, recurring_api.skip),
    Route('POST', r'/subscriptions/(?P<subscription_id>[^/]+)/unskip', CUSTOMER, recurring_api.unskip),
    Route('POST', r'/subscriptions/(?P<subscription_id>[^/]+)/change-plan', CUSTOMER, recurring_api.change_plan),
    Route('POST', r'/subscriptions/(?P<subscription_id>[^/]+)/discard-change', CUSTOMER, recurring_api.discard_change),
    Route('PUT', r'/subscriptions/(?P<subscription_id>[^/]+)/checkout', CUSTOMER, recurring_api.change_checkout),
    Route('POST', r'/subscriptions/(?P<subscription_id>[^/]+)/stop', CUSTOMER, recurring_api.stop),
    Route('POST', r'/admin/recurring/plan', ADMIN, recurring_api.plan_due),
    Route('POST', r'/admin/recurring/cycles/(?P<cycle_id>[^/]+)/preview', ADMIN, recurring_api.preview),
    Route('POST', r'/admin/recurring/cycles/(?P<cycle_id>[^/]+)/execute', ADMIN, recurring_api.execute),
    Route('POST', r'/admin/recurring/cycles/(?P<cycle_id>[^/]+)/hold', ADMIN, recurring_api.hold),
    Route('POST', r'/admin/recurring/cycles/(?P<cycle_id>[^/]+)/release', ADMIN, recurring_api.release),
    Route('GET', r'/recurring/cycles/(?P<cycle_id>[^/]+)', CUSTOMER, recurring_api.cycle_show),
    Route('GET', r'/admin/recurring/report', ADMIN, recurring_api.report),
)


def resolve_route(method, path):
    for route in ROUTES:
        params = route.match(method, path)
        if params is not None:
            return route, params
    raise DomainError("route_not_found", "No route matches this request", 404)
