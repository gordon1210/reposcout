from tests.support import CommerceTest


class ReconciliationTests(CommerceTest):
    def test_inventory_reconciles_after_transfer_ship_cancel_and_return(self):
        self.receive(6)
        self.ok("POST", "/stock/transfer", {"sku": "TEA", "source": "north", "destination": "south", "quantity": 2}, "a-warehouse")
        order = self.order(4)
        shipment = self.ship(order["order_id"], 2)
        self.cancel(order["order_id"], 2)
        self.ok("POST", f"/orders/{order['order_id']}/returns", {"shipment_id": shipment["shipment_id"], "quantity": 1, "days": 2, "reason": "unwanted"})
        report = self.ok("GET", "/reports/stock", token="a-admin")
        self.assertTrue(report["reconciliation"]["consistent"], report)
        self.assertEqual((report["total_on_hand"], report["total_reserved"]), (5, 0))
