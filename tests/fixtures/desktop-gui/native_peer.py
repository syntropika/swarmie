#!/usr/bin/env python3
"""Deterministic native Codex protocol and stateless quota peer for desktop QA."""
import json
import os
import sys


def record(value):
    with open(os.environ["SWARMIE_GUI_PEER_LEDGER"], "a", encoding="utf-8") as output:
        output.write(json.dumps(value) + "\n")


record({"peer": "codex", "argv": sys.argv[1:]})
thread = "native-acceptance-thread"
for line in sys.stdin:
    request = json.loads(line)
    method, params = request.get("method"), request.get("params", {})
    record({"peer": "codex", "method": method, "params": params})
    if method == "initialized":
        continue
    if method == "initialize":
        result = {}
    elif method == "account/rateLimits/read":
        result = {"rateLimitsByLimitId": {"codex": {"planType": "fixture", "primary": {"usedPercent": 25, "windowDurationMins": 300, "resetsAt": 2000000000}}}}
    elif method in ("thread/start", "thread/resume"):
        result = {"thread": {"id": thread}, "model": "fixture-model", "approvalPolicy": "on-request", "sandbox": {"type": "readOnly"}}
    elif method == "model/list":
        result = {"data": [{"id": "fixture-model", "model": "fixture-model", "displayName": "Fixture model"}]}
    elif method == "skills/list":
        result = {"data": [{"skills": [{"name": "acceptance", "description": "Native acceptance skill", "path": ".agents/skills/acceptance/SKILL.md", "enabled": True}]}]}
    elif method == "turn/start":
        result = {"turn": {"id": "fixture-turn"}}
    elif method == "thread/read":
        result = {"thread": {"id": thread, "turns": []}}
    else:
        print(json.dumps({"jsonrpc": "2.0", "id": request.get("id"), "error": {"code": -32601, "message": "Deterministic fixture does not implement " + str(method)}}), flush=True)
        continue
    print(json.dumps({"jsonrpc": "2.0", "id": request["id"], "result": result}), flush=True)
    if method == "turn/start":
        for name, body in [("turn/started", {"threadId": thread, "turn": {"id": "fixture-turn"}}), ("item/agentMessage/delta", {"threadId": thread, "itemId": "fixture-native-message", "delta": "Native peer completed the fixture prompt."}), ("turn/completed", {"threadId": thread, "turn": {"id": "fixture-turn", "status": "completed"}})]:
            print(json.dumps({"jsonrpc": "2.0", "method": name, "params": body}), flush=True)
