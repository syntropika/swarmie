#!/usr/bin/env python3
"""Fail closed if an account query submits a conversation or model operation."""
import json
import os
import sys

provider = os.environ["SWARMIE_QUOTA_PROVIDER"]
ledger = os.environ["SWARMIE_QUOTA_LEDGER"]
with open(ledger, "a") as output:
    output.write(json.dumps({"argv": sys.argv[1:], "home": os.environ.get("CODEX_HOME") or os.environ.get("CLAUDE_CONFIG_DIR")}) + "\n")
for line in sys.stdin:
    request = json.loads(line)
    method = request.get("method") if provider == "codex" else request.get("request", {}).get("subtype")
    with open(ledger, "a") as output:
        output.write(json.dumps({"method": method, "request": request}) + "\n")
    if method == "initialized":
        continue
    if method == "initialize":
        value = {}
    elif method == "account/rateLimits/read":
        value = {"rateLimitsByLimitId": {"codex": {"planType": "fixture", "primary": {"usedPercent": 25, "windowDurationMins": 300, "resetsAt": 2000000000}}}}
    elif method == "get_usage":
        value = {"rate_limits": {"five_hour": {"utilization": 40, "resets_at": "2033-05-18T03:33:20Z"}}}
    else:
        raise AssertionError("A quota query attempted a non-account operation: " + str(method))
    if os.environ.get("SWARMIE_QUOTA_EMPTY") and method != "initialize":
        value = {}
    if os.environ.get("SWARMIE_QUOTA_FAIL") and method != "initialize":
        if provider == "codex":
            response = {"jsonrpc": "2.0", "id": request["id"], "error": {"code": -32000, "message": "account unavailable"}}
        else:
            response = {"type": "control_response", "response": {"subtype": "error", "request_id": request["request_id"], "error": "account unavailable"}}
    elif provider == "codex":
        response = {"jsonrpc": "2.0", "id": request["id"], "result": value}
    else:
        response = {"type": "control_response", "response": {"subtype": "success", "request_id": request["request_id"], "response": value}}
    print(json.dumps(response), flush=True)
