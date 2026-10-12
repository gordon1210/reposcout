from tests.support import CommerceTest


class CustomerAddressTests(CommerceTest):
    def test_delivery_region_follows_validated_address(self):
        self.ok("PUT", "/account/address", {"name": "Alice", "street": "2 Main Street", "postal_code": "10001", "country": "us"})
        result = self.ok("POST", "/quotes", {"items": [{"sku": "MUG", "quantity": 1}]})
        self.assertEqual((result["region"], result["tax_cents"], result["total_cents"]), ("international", 0, 2199))

    def test_invalid_change_preserves_previous_country(self):
        response = self.raw("PUT", "/account/address", {"name": "Alice", "street": "", "postal_code": "1", "country": "DE"})
        self.assertEqual(response["status"], 422)
        self.assertEqual(self.ok("GET", "/account")["country"], "DE")
