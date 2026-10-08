import json
import sys
sys.path.insert(0, sys.argv[1])
from api import dispatch
print(json.dumps({path: [dispatch(path, {"days": day})["allowed"] for day in (13, 14, 15)]
                  for path in ("/refund", "/support/refund")}))
