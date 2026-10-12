import json
import sys
sys.path.insert(0, sys.argv[1])
from meridian.bootstrap import demo_application


def ok(app, method, path, body=None, role="a-customer"):
    result = app.request(method, path, role, body)
    if result["status"] != 200:
        raise AssertionError((method, path, result))
    return result["data"]


def receive(app, units, warehouse="north"):
    ok(app, "POST", "/stock/receive", {"sku": "TEA", "warehouse": warehouse, "quantity": units}, "a-warehouse")


def create(app, units, backorder=False):
    return ok(app, "POST", "/orders", {"items": [{"sku": "TEA", "quantity": units}], "allow_backorder": backorder, "pickup": True})["order_id"]


def ship(app, order_id, units):
    return ok(app, "POST", f"/warehouse/orders/{order_id}/ship", {"sku": "TEA", "quantity": units}, "a-warehouse")["shipment_id"]


def returned(app, order_id, shipment_id, units):
    return ok(app, "POST", f"/orders/{order_id}/returns", {"shipment_id": shipment_id, "quantity": units, "days": 5, "reason": "unwanted"})


def observation(app, order_id):
    order = ok(app, "GET", f"/orders/{order_id}")
    stock = ok(app, "GET", "/stock/TEA", role="a-warehouse")
    line = order["lines"][0]
    return {"status": order["status"], "counts": [line[name] for name in
            ("ordered", "shipped", "cancelled", "returned", "reserved", "unallocated")],
            "stock": [stock["on_hand"], stock["reserved"], stock["available"]],
            "invoice": [order["invoice"][name] for name in ("total_cents", "credited_cents", "due_cents")]}


app = demo_application()
order_id = create(app, 4, True)
states = [observation(app, order_id)]
receive(app, 3)
ok(app, "POST", f"/warehouse/orders/{order_id}/allocate", role="a-warehouse")
states.append(observation(app, order_id))
first_shipment = ship(app, order_id, 1)
states.append(observation(app, order_id))
ok(app, "POST", f"/orders/{order_id}/cancel", {"sku": "TEA", "quantity": 1})
states.append(observation(app, order_id))
receive(app, 1, "south")
ok(app, "POST", f"/warehouse/orders/{order_id}/allocate", role="a-warehouse")
states.append(observation(app, order_id))
last_shipment = ship(app, order_id, 2)
states.append(observation(app, order_id))
returned(app, order_id, first_shipment, 1)
states.append(observation(app, order_id))
returned(app, order_id, last_shipment, 2)
states.append(observation(app, order_id))
report = ok(app, "GET", "/reports/fulfillment", role="a-admin")
other = demo_application()
receive(other, 2)
cancelled_id = create(other, 2)
ok(other, "POST", f"/orders/{cancelled_id}/cancel", {"sku": "TEA", "quantity": 2})
all_cancelled = observation(other, cancelled_id)
third = demo_application()
receive(third, 3)
open_id = create(third, 3)
partial_shipment = ship(third, open_id, 2)
returned(third, open_id, partial_shipment, 1)
partial_return_open = observation(third, open_id)
preview_app = demo_application()
quote_discounts = []
for code in ("WELCOME", "welcome-team", "WELCOME-team"):
    quote_discounts.append(ok(preview_app, "POST", "/quotes", {"items": [{"sku": "TEA", "quantity": 2}], "promotion": code, "pickup": True})["discount_cents"])
preview = ok(preview_app, "POST", "/admin/promotion-preview", {"code": " welcome-team ", "subtotal_cents": 200}, "a-admin")
print(json.dumps({"states": states, "all_cancelled": all_cancelled, "partial_return_open": partial_return_open,
                  "report": report, "customer_discounts": quote_discounts,
                  "staff_preview": {"eligible": preview["eligible"], "discount_cents": preview["discount_cents"]}}, sort_keys=True))
