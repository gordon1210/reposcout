import json
import sys
sys.path.insert(0, sys.argv[1])
from meridian.bootstrap import demo_application

app = demo_application()


def event(tenant_label, provider, event_id, quantity, **extra):
    body = {"event_id": event_id, "type": "stock.received",
            "data": {"sku": "TEA", "warehouse": "north", "quantity": quantity}, **extra}
    response = app.request("POST", f"/events/{provider}", f"{tenant_label}-provider", body)
    return {"status": response["status"], "applied": response.get("data", {}).get("applied"),
            "duplicate": response.get("data", {}).get("duplicate")}


def stock(tenant):
    response = app.request("GET", "/stock/TEA", f"{tenant}-warehouse")
    if response["status"] != 200:
        raise AssertionError(response)
    return {name: response["data"][name] for name in ("on_hand", "reserved", "available")}


results = [event("a", "acme", "shared-receipt", 7),
           event("b", "acme", "shared-receipt", 11),
           event("a", "acme", "shared-receipt", 7),
           event("a", "backup", "shared-receipt", 3)]
after_collision = {"a": stock("a"), "b": stock("b")}
results.append(event("a", "acme", "correctable", 0))
results.append(event("a", "acme", "correctable", 4))
results.append(event("b", "acme", "trusted-context", 2, tenant="tenant-a", tenant_id="tenant-a"))
def reconcile(tenant_label, event_ids, provider="acme"):
    body = {"events": [{"event_id": event_id, "type": "stock.received"} for event_id in event_ids]}
    response = app.request("POST", f"/admin/events/{provider}/reconcile", f"{tenant_label}-admin", body)
    if response["status"] != 200:
        raise AssertionError(response)
    view = response["data"]
    return {"applied_ids": [row["event_id"] for row in view["applied"]],
            "missing_ids": view["missing"], "complete": view["complete"]}


reconciliation = {"a": reconcile("a", ["shared-receipt", "correctable", "not-yet-delivered"]),
                  "b": reconcile("b", ["shared-receipt", "trusted-context"]),
                  "backup": reconcile("a", ["shared-receipt"], "backup")}
print(json.dumps({"responses": results, "after_collision": after_collision,
                  "final_stock": {"a": stock("a"), "b": stock("b")},
                  "reconciliation": reconciliation}, sort_keys=True))
