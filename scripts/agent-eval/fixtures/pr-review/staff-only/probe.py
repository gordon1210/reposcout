import json
import sys
sys.path.insert(0, sys.argv[1])
from api import dispatch
print(json.dumps({route: [dispatch(route, {"code": code, "subtotal_cents": subtotal})["discount_cents"]
                         for code, subtotal in [("WELCOME", 1999), ("WELCOME", 2000), ("WELCOME-X", 2000)]]
                  for route in ("/storefront/discount", "/staff/discount")}))
