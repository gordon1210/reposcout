import json
import sys
sys.path.insert(0, sys.argv[1])
from meridian.bootstrap import demo_application


def ok(app, method, path, body=None, role="a-customer"):
    response = app.request(method, path, role, body)
    if response["status"] != 200:
        raise AssertionError((method, path, response))
    return response["data"]


def receive(app, count, warehouse):
    return ok(app, "POST", "/stock/receive", {"sku": "TEA", "warehouse": warehouse, "quantity": count}, "a-warehouse")


def order(app, count, backorder=False):
    return ok(app, "POST", "/orders", {"items": [{"sku": "TEA", "quantity": count}], "allow_backorder": backorder, "pickup": True})


def show(app, order_id):
    return ok(app, "GET", f"/orders/{order_id}")


def stock(app):
    return ok(app, "GET", "/stock/TEA", role="a-warehouse")


def ledger(app, order_id):
    return ok(app, "GET", "/inventory/ledger", {"order_id": order_id, "kind": "release"}, "a-warehouse")["entries"]


def snapshot(app, order_id):
    return {"order": show(app, order_id), "stock": stock(app), "ledger": ledger(app, order_id)}


def shape(view, available):
    line = view["lines"][0]
    return {name: line[name] for name in ("ordered", "shipped", "cancelled", "reserved")} | {"available": available}


app = demo_application()
receive(app, 6, "north")
created = order(app, 10, True)
order_id = created["order_id"]
ok(app, "POST", f"/warehouse/orders/{order_id}/ship", {"sku": "TEA", "quantity": 4}, "a-warehouse")
ok(app, "POST", f"/orders/{order_id}/cancel", {"sku": "TEA", "quantity": 2})
receive(app, 2, "south")
ok(app, "POST", f"/warehouse/orders/{order_id}/allocate", role="a-warehouse")
before = snapshot(app, order_id)
controls = {}
for name, count in (("zero", 0), ("over", 5), ("negative", -1), ("boolean", True)):
    response = app.request("POST", f"/orders/{order_id}/cancel", "a-customer", {"sku": "TEA", "quantity": count})
    controls[name] = {"status": response["status"], "unchanged": snapshot(app, order_id) == before}
result = ok(app, "POST", f"/orders/{order_id}/cancel", {"sku": "TEA", "quantity": 1})
new_entries = ledger(app, order_id)[len(before["ledger"]):]
after = shape(result["order"], stock(app)["available"])
after.update(cancelled_quantity=result["cancelled_quantity"],
             release_units=-sum(entry["reserved_delta"] for entry in new_entries),
             released_by_warehouse=[[entry["warehouse"], -entry["reserved_delta"]] for entry in new_entries],
             shipped_history=[[row["warehouse"], row["shipped"], row["released"]]
                              for row in result["order"]["reservations"] if row["shipped"]])
other = demo_application()
receive(other, 3, "north")
control_id = order(other, 3)["order_id"]
ok(other, "POST", f"/warehouse/orders/{control_id}/ship", {"sku": "TEA", "quantity": 1}, "a-warehouse")
control = ok(other, "POST", f"/orders/{control_id}/cancel", {"sku": "TEA", "quantity": 2})
controls["same_warehouse_full"] = {
    "cancelled": control["order"]["lines"][0]["cancelled"],
    "shipped": control["order"]["lines"][0]["shipped"],
    "reserved": control["order"]["lines"][0]["reserved"],
    "available": stock(other)["available"],
    "release_units": -sum(entry["reserved_delta"] for entry in ledger(other, control_id)),
}
print(json.dumps({"before": shape(before["order"], before["stock"]["available"]), "after": after, "controls": controls}, sort_keys=True))
