import json
import sys
sys.path.insert(0, sys.argv[1])
from api import dispatch
from reporting import headings, page
from telemetry import summarize
print(json.dumps({"responses": [dispatch("POST /checkout", {"items": [{"quantity": q, "unit_price_cents": p}]})
                                 for q, p in [(2, 1250), (1, 99)]],
                  "noise": [headings(["unit_price", "total"]), page([1, 2, 3], 2, 2),
                            summarize([{"route": "/a", "status": 200}, {"route": "/a", "status": 200}])]}))
