# Request contracts

Every success is `{ "status": 200, "data": ... }`. A rejection is `{ "status": <code>,
"error": { "code": <stable name>, "message": <explanation> } }`. Pass a Python dictionary as
the body even for filtered GET requests. IDs returned by one request can be used directly in
the next. All routes and actual bindings are listed in `meridian/api/routes.py`.

The demonstration credentials are `a-customer`, `a-warehouse`, `a-admin`, `a-provider` and the
corresponding `b-` credentials for a second tenant. Customer identities are Alice and Bob.
Provider credentials permit `acme` and `backup`. Both tenants have north then south warehouse
priority. Their stock starts empty.

| Request | Principal body fields | Role |
| --- | --- | --- |
| GET `/catalog`, `/catalog/{sku}` | `query`, `category`, `limit`, `after` | customer |
| POST `/quotes`, `/orders` | `items: [{sku, quantity}]`, `promotion`, `pickup`, `express`, `allow_backorder` | customer |
| POST `/baskets` | none | customer |
| PUT `/baskets/{id}/items` | `sku`, `quantity`, `version` | customer |
| POST `/baskets/{id}/quote`, `/checkout` | pricing options, current `version` for checkout | customer |
| GET `/orders`, `/orders/{id}` | pagination on list | customer |
| POST `/orders/{id}/cancel` | `sku`, `quantity` | customer |
| POST `/orders/{id}/returns` | `shipment_id`, `quantity`, `days`, `reason` | customer |
| GET `/orders/{id}/credits` | none | customer |
| POST `/orders/{id}/notes` | `text` | customer |
| GET `/account`, `/account/statement` | none | customer |
| PUT `/account/address`, `/account/preferences` | validated address or preference fields | customer |
| GET `/stock/{sku}` | none | warehouse/admin |
| POST `/stock/receive` | `sku`, `warehouse`, `quantity` | warehouse/admin |
| POST `/stock/count` | `sku`, `warehouse`, `counted`, `reason` | warehouse/admin |
| POST `/stock/transfer` | `sku`, `source`, `destination`, `quantity` | warehouse/admin |
| POST `/warehouse/orders/{id}/allocate` | none | warehouse/admin |
| POST `/warehouse/orders/{id}/ship` | `sku`, `quantity`, optional `warehouse` | warehouse/admin |
| GET `/warehouse/{warehouse}/picklist` | none | warehouse/admin |
| GET `/inventory/ledger` | optional `order_id`, `sku`, `kind` filters | warehouse/admin |
| POST `/events/{provider}` | `event_id`, `type`, `data` | provider |
| GET `/reports/{report}` | report is stock/sales/customers/fulfillment/finance | admin |
| POST `/admin/promotion-preview` | `code`, `subtotal_cents` | admin |
| GET `/admin/messages`, `/admin/audit` | optional audit `action` | admin |
| POST `/admin/outbox/drain` | `tick`, optional `simulate_failure` | admin |

Product creation, retirement/repricing, customer registration and internal order notes are also
registered administrative routes. Staff preview has a separate handler and is deliberately
informational; it is never called by customer quote or order creation.
