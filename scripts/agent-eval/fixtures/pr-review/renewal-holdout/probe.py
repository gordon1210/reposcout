import json
import sys
sys.path.insert(0, sys.argv[1])
from service import dispatch
print(json.dumps({kind: [dispatch(kind, {"hours": age})["renewed"] for age in (23, 24, 25)]
                  for kind in ("manual", "scheduled")}))
