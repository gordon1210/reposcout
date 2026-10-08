import json
import sys
sys.path.insert(0, sys.argv[1])
from api import dispatch
print(json.dumps([dispatch("/shipping/quote", {"region": region})["shipping_fee_cents"]
                  for region in ("domestic", "international")]))
