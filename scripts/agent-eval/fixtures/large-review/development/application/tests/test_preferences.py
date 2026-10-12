from tests.support import CommerceTest


class PreferenceTests(CommerceTest):
    def test_preferences_merge_independently(self):
        self.ok("PUT", "/account/preferences", {"language": "de"})
        result = self.ok("PUT", "/account/preferences", {"email_receipts": False})
        self.assertEqual(result, {"language": "de", "email_receipts": False})

    def test_unknown_or_wrongly_typed_preferences_reject(self):
        for body in ({"currency": "USD"}, {"email_receipts": "false"}, {"language": "xx"}):
            self.assertEqual(self.raw("PUT", "/account/preferences", body)["status"], 422)
