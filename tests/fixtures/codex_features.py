#!/usr/bin/env python3
"""Native protocol peer with persisted history and deliberately different model contracts."""
import json
import os
import sys
import time
import uuid

ledger_path = os.environ["MPAI_FIXTURE_LEDGER"]
thread = None
pending_child = None


def load():
    try:
        with open(ledger_path) as source:
            return json.load(source)
    except FileNotFoundError:
        return {}


def save(ledger):
    with open(ledger_path, "w") as target:
        json.dump(ledger, target)


def emit(frame):
    print(json.dumps(frame), flush=True)


def response(request, result):
    emit({"jsonrpc": "2.0", "id": request["id"], "result": result})


def notify(method, params):
    emit({"jsonrpc": "2.0", "method": method, "params": params})


def complete(text, turn_id):
    ledger = load()
    item = {"id": "message-" + turn_id, "type": "agentMessage", "text": text}
    ledger[thread].append({"id": turn_id, "status": "completed", "items": [item]})
    save(ledger)
    notify("item/agentMessage/delta", {"threadId": thread, "itemId": item["id"], "delta": text})
    notify("item/completed", {"threadId": thread, "item": item})
    notify("turn/completed", {"threadId": thread, "turn": {"id": turn_id, "status": "completed"}})


for line in sys.stdin:
    request = json.loads(line)
    method = request.get("method")
    params = request.get("params", {})
    if "result" in request and request.get("id") == "child-question":
        assert request["result"]["answers"]["target"]["answers"] == ["Linux"]
        complete("child-answer-received", pending_child)
    elif method == "initialize":
        response(request, {})
    elif method in ("thread/start", "thread/resume", "thread/fork"):
        ledger = load()
        if method == "thread/resume":
            thread = params["threadId"]
        else:
            thread = str(uuid.uuid4())
            history = ledger.get(params.get("threadId"), [])
            if params.get("lastTurnId"):
                cut = next(i for i, turn in enumerate(history) if turn["id"] == params["lastTurnId"])
                history = history[:cut + 1]
            ledger[thread] = history if method == "thread/fork" else []
            save(ledger)
        response(request, {"thread": {"id": thread}, "model": params.get("model") or "reasoning", "approvalPolicy": "on-request", "approvalsReviewer": params.get("approvalsReviewer", "user"), "sandbox": {"type": "readOnly"}, "reasoningEffort": "medium"})
    elif method == "thread/read":
        response(request, {"thread": {"id": params["threadId"], "turns": load()[params["threadId"]]}})
    elif method == "model/list":
        response(request, {"data": [
            {"id": "reasoning", "model": "reasoning", "displayName": "Reasoning", "defaultReasoningEffort": "medium", "supportedReasoningEfforts": [{"reasoningEffort": "low"}, {"reasoningEffort": "medium"}, {"reasoningEffort": "high"}], "serviceTiers": [{"id": "priority", "name": "Fast"}]},
            {"id": "low-only", "model": "low-only", "displayName": "Low only", "defaultReasoningEffort": "low", "supportedReasoningEfforts": [{"reasoningEffort": "low"}]}
        ]})
    elif method == "skills/list":
        response(request, {"data": [{"skills": [{"name": "review", "description": "Review changes", "path": "/fixture/review/SKILL.md", "enabled": True}, {"name": "disabled", "enabled": False}]}]})
    elif method == "account/rateLimits/read":
        response(request, {"rateLimits": {"limitId": "other", "primary": {"usedPercent": 99}}, "rateLimitsByLimitId": {"codex": {"planType": "fixture", "primary": {"usedPercent": 25, "windowDurationMins": 300, "resetsAt": 2000000000}}}, "rateLimitResetCredits": {"availableCount": 0}})
    elif method == "turn/start":
        turn_id = str(uuid.uuid4())
        response(request, {"turn": {"id": turn_id}})
        notify("turn/started", {"threadId": thread, "turn": {"id": turn_id}})
        prompt = params["input"][0]["text"]
        if prompt == "child-question":
            pending_child = turn_id
            emit({"jsonrpc": "2.0", "id": "child-question", "method": "item/tool/requestUserInput", "params": {"threadId": "child", "questions": [{"id": "target", "question": "Target?", "options": [{"label": "Linux"}]}]}})
        elif prompt == "children":
            notify("item/completed", {"threadId": thread, "item": {"id": "spawn-child", "type": "collabAgentToolCall", "tool": "spawnAgent", "receiverThreadIds": ["child"], "model": "child-model"}})
            notify("turn/started", {"threadId": "child", "turn": {"id": "child-turn"}})
            notify("item/agentMessage/delta", {"threadId": "child", "itemId": "child-message", "delta": "child-only"})
            notify("turn/completed", {"threadId": "child", "turn": {"id": "child-turn", "status": "completed"}})
            notify("thread/tokenUsage/updated", {"threadId": "child", "tokenUsage": {"total": {"totalTokens": 7}}})
            complete("parent-only", turn_id)
        elif prompt == "execution":
            complete(json.dumps({"arguments": sys.argv[1:], "home": os.environ.get("CODEX_HOME"), "value": os.environ.get("MPAI_TEST_VALUE"), "headers": {key: value for key, value in os.environ.items() if key.startswith("MPAI_MCP_")}, "turn": params}), turn_id)
        elif prompt == "tool-failures":
            for tool_id, status, exit_code in [("denied", "declined", None), ("cancelled", "cancelled", None), ("failed", "completed", 2)]:
                notify("item/started", {"threadId": thread, "item": {"id": tool_id, "type": "commandExecution", "command": "fixture", "status": "inProgress"}})
                notify("item/completed", {"threadId": thread, "item": {"id": tool_id, "type": "commandExecution", "status": status, "exitCode": exit_code}})
            complete("tool receipts", turn_id)
        elif prompt == "reroute":
            notify("model/rerouted", {"threadId":thread,"turnId":turn_id,"fromModel":"reasoning","toModel":"safety-model","reason":"highRiskCyberActivity"})
            complete("rerouted turn", turn_id)
        else:
            complete(json.dumps(params), turn_id)
    elif method == "thread/compact/start":
        response(request, {})
        time.sleep(0.15)
        notify("item/completed", {"threadId": thread, "item": {"id": "compact", "type": "contextCompaction"}})
    elif method == "thread/rollback":
        ledger = load()
        ledger[thread] = ledger[thread][:-params["numTurns"]]
        save(ledger)
        response(request, {"thread": {"id": thread}})
