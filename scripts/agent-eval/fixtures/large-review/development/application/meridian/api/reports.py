from meridian.core.errors import require
from meridian.reporting.stock import stock_report
from meridian.reporting.sales import sales_report
from meridian.reporting.customers import customer_report
from meridian.reporting.fulfillment import fulfillment_report
from meridian.reporting.ledger import finance_report

REPORTS = {
    "sales": lambda app, tenant: sales_report(app.state, tenant),
    "customers": lambda app, tenant: customer_report(app.state, tenant),
    "fulfillment": lambda app, tenant: fulfillment_report(app.state, tenant),
    "finance": lambda app, tenant: finance_report(app.state, tenant),
    "stock": lambda app, tenant: stock_report(app.state, app.settings, tenant),
}


def show(app, context, body, params):
    report = REPORTS.get(params["report"])
    require(report is not None, "report_not_found", "Unknown report", 404)
    return report(app, context.tenant)
