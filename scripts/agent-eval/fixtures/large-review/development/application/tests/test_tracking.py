from tests.support import CommerceTest


class TrackingTests(CommerceTest):
    def test_provider_tracking_moves_forward_only(self):
        self.receive(1)
        order = self.order(1)
        shipment = self.ship(order["order_id"], 1)
        data = {"shipment_id": shipment["shipment_id"], "status": "delivered", "location": "Front desk"}
        self.assertEqual(self.event("delivery-1", "shipment.tracked", data)["status"], 200)
        data["status"] = "in_transit"
        self.assertEqual(self.event("delivery-2", "shipment.tracked", data)["status"], 409)
        report = self.ok("GET", "/reports/fulfillment", token="a-admin")
        self.assertEqual(report["delivered_count"], 1)
