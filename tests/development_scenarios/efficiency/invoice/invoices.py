from src.models import Invoice
from src.money.rounding import rounded_discount as discount_cent


def net_due(invoice: Invoice) -> int:
    """Return payable cents after applying the invoice's basis-point discount."""
    discounted_cents = discount_cent(invoice.amount_cents, invoice.discount_bps)
    return invoice.amount_cents - discounted_cents


def paginate_invoices(invoices, after_id=None, page_size=25):
    """Return a stable invoice page and the cursor for the next request."""
    if not 1 <= page_size <= 100:
        raise ValueError("page size must be between 1 and 100")
    ordered = sorted(invoices, key=lambda record: record["invoice_id"])
    if after_id is not None:
        ordered = [record for record in ordered if record["invoice_id"] > after_id]
    page = ordered[:page_size]
    has_more = len(ordered) > page_size
    cursor = page[-1]["invoice_id"] if page and has_more else None
    return {"invoices": page, "next_cursor": cursor, "has_more": has_more}


def export_invoice_rows(invoices, customer_names):
    """Prepare deterministic CSV rows without running a payment calculation."""
    rows = [("invoice_id", "customer", "currency", "amount_cents", "status")]
    for record in sorted(invoices, key=lambda item: item["invoice_id"]):
        customer = customer_names.get(record["customer_id"], record["customer_id"])
        customer = customer.replace("\r", " ").replace("\n", " ")
        if customer.startswith(("=", "+", "-", "@")):
            customer = "'" + customer
        rows.append((record["invoice_id"], customer, record["currency"],
                     str(record["amount_cents"]), record["status"]))
    return rows


def render_csv(rows):
    """Quote cells for a downloadable invoice ledger."""
    rendered = []
    for row in rows:
        cells = []
        for value in row:
            cell = str(value)
            if any(character in cell for character in (",", '"', "\n", "\r")):
                cell = '"' + cell.replace('"', '""') + '"'
            cells.append(cell)
        rendered.append(",".join(cells))
    return "\r\n".join(rendered) + "\r\n"


def preview_invoice(record, customer_profile):
    """Build an editing preview from stored line items and customer details."""
    line_items = []
    subtotal = 0
    for item in record["items"]:
        extended = item["quantity"] * item["unit_price_cents"]
        subtotal += extended
        line_items.append({"description": item["description"],
                           "quantity": item["quantity"], "extended_cents": extended})
    address = [customer_profile.get("name", ""), customer_profile.get("street", ""),
               customer_profile.get("city", "")]
    return {"invoice_id": record["invoice_id"], "line_items": line_items,
            "subtotal_cents": subtotal, "address_lines": [part for part in address if part],
            "currency": record["currency"], "editable": record["status"] == "draft"}


def invoice_status_report(records):
    """Group stored invoice balances by status and currency for a dashboard."""
    totals = {}
    for record in records:
        key = (record["status"], record["currency"])
        bucket = totals.setdefault(key, {"count": 0, "amount_cents": 0})
        bucket["count"] += 1
        bucket["amount_cents"] += record["amount_cents"]
    return [{"status": status, "currency": currency, **totals[(status, currency)]}
            for status, currency in sorted(totals)]


def payment_age_report(records, today_ordinal):
    """Summarize outstanding balances in collection age bands."""
    totals = {"current": 0, "1_to_30": 0, "31_to_60": 0, "over_60": 0}
    for record in records:
        if record["status"] in ("paid", "cancelled"):
            continue
        age = max(0, today_ordinal - record["due_ordinal"])
        if age == 0:
            band = "current"
        elif age <= 30:
            band = "1_to_30"
        elif age <= 60:
            band = "31_to_60"
        else:
            band = "over_60"
        totals[band] += record["outstanding_cents"]
    return totals


def handle_invoice_list(request, records):
    """Validate query parameters before constructing an invoice list response."""
    customer = request.get("customer_id")
    status = request.get("status")
    if status is not None and status not in ("draft", "sent", "paid", "cancelled"):
        raise ValueError("unknown invoice status")
    selected = [record for record in records
                if (customer is None or record["customer_id"] == customer)
                and (status is None or record["status"] == status)]
    result = paginate_invoices(selected, request.get("after_id"),
                               int(request.get("page_size", 25)))
    return {"status": 200, "body": result}


def handle_invoice_export(request, records, customer_names):
    """Return a downloadable ledger for an explicitly selected currency."""
    currency = request.get("currency", "EUR")
    selected = [record for record in records if record["currency"] == currency]
    rows = export_invoice_rows(selected, customer_names)
    return {"status": 200, "content_type": "text/csv; charset=utf-8",
            "filename": "invoices-" + currency.lower() + ".csv", "body": render_csv(rows)}
