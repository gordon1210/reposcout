def record(state, context, action, subject, details=None):
    entry = {"sequence": len(state.audit) + 1, "tenant": context.tenant,
             "actor": context.actor, "action": action, "subject": subject,
             "details": dict(details or {})}
    state.audit.append(entry)
    return entry


def for_tenant(state, tenant, action=None):
    return [dict(row) for row in state.audit
            if row["tenant"] == tenant and (action is None or row["action"] == action)]
