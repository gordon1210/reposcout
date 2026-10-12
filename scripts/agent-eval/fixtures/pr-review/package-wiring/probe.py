import json
from pathlib import Path
import subprocess
import sys

target = (Path(sys.argv[1]) / 'api.js').as_uri()
script = 'import { dispatch } from ' + json.dumps(target) + '; console.log(JSON.stringify(["domestic","international"].map(region => dispatch("/shipping/quote",{region}).shipping_fee_cents)));'
result = subprocess.run(['node', '--input-type=module', '--eval', script], check=True,
                        capture_output=True, text=True, timeout=5)
print(result.stdout.strip())
