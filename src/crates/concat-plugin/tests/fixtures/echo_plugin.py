#!/usr/bin/env python3
"""A minimal, well-behaved plugin used by the loader + bridge end-to-end test.

It speaks the JSON-RPC envelope over stdin/stdout, one line per frame:

* the host's `plugin.hello` gets a `ready` result echoing the id and api;
* any later call gets a result echoing the method name, so a round trip
  through `bridge::serve` can be asserted.

It never writes to stderr and never exits on its own: the test closes stdin
to end the session, which is how a host ends a plugin cleanly.
"""

import json
import sys

API_MAJOR = 1

for line in sys.stdin:
    line = line.strip()
    if not line:
        continue
    try:
        frame = json.loads(line)
    except json.JSONDecodeError:
        continue

    call_id = frame.get("id")
    method = frame.get("method", "")

    if method == "plugin.hello":
        reply = {
            "jsonrpc": "2.0",
            "id": call_id,
            "result": {"api": API_MAJOR},
        }
    else:
        reply = {
            "jsonrpc": "2.0",
            "id": call_id,
            "result": {"method": method},
        }

    sys.stdout.write(json.dumps(reply) + "\n")
    sys.stdout.flush()
