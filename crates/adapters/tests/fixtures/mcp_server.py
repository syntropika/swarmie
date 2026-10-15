#!/usr/bin/env python3
"""Independent MCP peer for native transport acceptance; never requires a model account."""
import json
import os
import sys
from pathlib import Path

ledger = os.environ.get("MCP_LEDGER")
if ledger:
    with Path(ledger).open("a") as stream:
        stream.write(json.dumps({"event": "started", "pid": os.getpid(), "credential": os.environ.get("MCP_TOKEN")}) + "\n")

for line in sys.stdin:
    request = json.loads(line)
    method = request.get("method")
    if ledger:
        with Path(ledger).open("a") as stream:
            stream.write(json.dumps({"method": method}) + "\n")
    if "id" not in request:
        continue
    if method == "initialize":
        result = {"protocolVersion": request["params"]["protocolVersion"], "serverInfo": {"name": "fixture", "version": "1.0"}, "capabilities": {"tools": {}}}
    elif method == "tools/list":
        result = {"tools": [{"name": "fixture_read", "description": "Read the fixture", "inputSchema": {"type": "object", "properties": {}}}]}
    elif method == "tools/call":
        result = {"content": [{"type": "text", "text": "fixture result"}]}
    else:
        result = {}
    print(json.dumps({"jsonrpc": "2.0", "id": request["id"], "result": result}), flush=True)
