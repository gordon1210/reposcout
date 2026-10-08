import json
import sys
sys.path.insert(0, sys.argv[1])
from api import dispatch
print(json.dumps([dispatch("POST /shipping/quote", {"delivery_pass": value})["shipping_fee_cents"]
                  for value in (True, False)]))
