import json
import sys
sys.path.insert(0, sys.argv[1])
from api import storage_status
print(json.dumps([storage_status({"used": used, "capacity": 1000})["status"]
                  for used in (0, 890, 895, 900, 1000)]))
