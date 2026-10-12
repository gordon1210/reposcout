import json
import sys
sys.path.insert(0, sys.argv[1])
from shipping import endpoint
print(json.dumps([endpoint({"region": region, "express": express})["shipping_fee_cents"]
                  for region, express in [("domestic", False), ("international", False),
                                          ("domestic", True), ("international", True)]]))
