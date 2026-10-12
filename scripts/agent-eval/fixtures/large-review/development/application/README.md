# Meridian commerce

Meridian is a dependency-free Python reference application for a tenant-scoped order and
fulfillment business. It implements catalog search, customer accounts, basket checkout,
stock allocation, shipment, cancellation, returns, invoicing, provider events and operational
reporting. Requests are ordinary Python calls; state lives in memory for the lifetime of an
`Application`. There is no HTTP server, database, background worker or external service.

```python
from meridian.bootstrap import demo_application

app = demo_application()
app.request("POST", "/stock/receive", "a-warehouse",
            {"warehouse": "north", "sku": "TEA", "quantity": 3})
order = app.request("POST", "/orders", "a-customer",
                    {"items": [{"sku": "TEA", "quantity": 2}], "pickup": True})
print(order["data"]["order_id"], order["data"]["status"])
```

Run the public checks from this directory:

```sh
python3 -B -m unittest discover -s tests
```

Python 3.10 or newer is sufficient. No installation or network access is needed. Source imports
work from the repository root. `meridian/bootstrap.py` supplies two distinct tenants, a small
catalog and trusted local credentials; these are synthetic demonstration identities.

Read [business rules](docs/business-rules.md), [request contracts](docs/request-contracts.md),
[fulfillment states](docs/fulfillment-states.md), [event delivery](docs/events.md), and
[architecture](docs/architecture.md) and [connected operations](docs/operations.md) for the supported behavior. Tests check observable request
results and rejection boundaries. Source navigation and review can start with the actual diff,
registered routes and the relevant workflow; reading every subsystem is not required.
