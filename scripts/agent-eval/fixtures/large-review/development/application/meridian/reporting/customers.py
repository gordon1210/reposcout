from meridian.billing.statements import customer_statement


def customer_report(state, tenant):
    rows = []
    for (owner, customer_id), customer in sorted(state.customers.items()):
        if owner != tenant:
            continue
        statement = customer_statement(state, tenant, customer_id)
        rows.append({"customer_id": customer_id, "order_count": len(statement["invoices"]),
                     "due_cents": statement["due_cents"], "wholesale": customer.wholesale})
    return {"customers": rows, "total_due_cents": sum(row["due_cents"] for row in rows)}
