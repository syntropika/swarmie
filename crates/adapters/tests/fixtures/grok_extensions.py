#!/usr/bin/env python3
"""Pinned native Grok ACP contracts without authentication or inference."""
import json
import pathlib
import sys

mode = sys.argv[1]
record = pathlib.Path.cwd() / "requests.jsonl"
active = None
stopped = False


def send(frame):
    print(json.dumps(frame), flush=True)


def task_rows():
    return [
        {"task_id": "own", "description": "Compile project", "command": "wrapped command",
         "display_command": "build", "kind": "bash", "completed": stopped,
         "explicitly_killed": stopped, "is_backgrounded": True, "owner_session_id": "native",
         "output": "compiler summary", "output_file": "/private/native-output", "exit_code": None},
        {"task_id": "sibling", "completed": False, "is_backgrounded": True, "owner_session_id": "other"},
        {"task_id": "foreground", "completed": False, "is_backgrounded": False},
    ]


for line in sys.stdin:
    frame = json.loads(line)
    with record.open("a") as out:
        out.write(json.dumps(frame) + "\n")
    method = frame.get("method")
    result = {}
    if method == "initialize":
        result = {"protocolVersion": 1, "agentCapabilities": {"loadSession": True},
                  "_meta": {"grokShell": mode != "no_marker", "agentVersion": "9.0.0" if mode == "future" else "1.0.52",
                            "availableCommands": [{"name": "compact", "description": "Summarize context"}]}}
    elif method in ("session/new", "session/load"):
        result = {"sessionId": "native", "models": {"currentModelId": "grok", "availableModels": [{"modelId": "grok", "name": "Grok"}]},
                  "modes": {"currentModeId": "code", "availableModes": [{"id": "code", "name": "Code"}]}}
    elif method == "_x.ai/interject":
        assert frame["params"]["sessionId"] == "native"
        result = {"result": {"status": "queued"}}
        send({"jsonrpc": "2.0", "method": "_x.ai/session_notification", "params": {
            "sessionId": "native", "update": {"sessionUpdate": "turn_completed", "usage": {
                "inputTokens": 100, "outputTokens": 25, "cachedReadTokens": 20,
                "cacheCreationTokens": 10, "costUsdTicks": 2500000000,
                "costIsPartial": mode == "partial"}}}})
    elif method == "_x.ai/rewind/points":
        result = {"rewind_points": [{"prompt_index": index} for index in range(3)]}
    elif method == "_x.ai/rewind/execute":
        assert frame["params"]["force"] is False
        result = {"success": mode != "conflict", "target_prompt_index": frame["params"]["targetPromptIndex"], "mode": frame["params"]["mode"]}
    elif method == "_x.ai/task/list":
        result = {"result": {"tasks": task_rows()}}
    elif method == "_x.ai/task/kill":
        assert frame["params"]["taskId"] == "own"
        assert frame["params"]["source"] == "clientUi"
        stopped = True
        result = {"result": {"taskId": "own", "outcome": "killed"}}
    elif method == "session/prompt":
        active = frame["id"]
        continue
    elif method == "session/cancel":
        if active:
            send({"jsonrpc": "2.0", "id": active, "result": {"stopReason": "cancelled"}})
            active = None
        continue
    elif method in ("session/set_model", "session/set_mode"):
        result = {}
    else:
        send({"jsonrpc": "2.0", "id": frame.get("id"), "error": {"code": -32601, "message": "Unexpected fixture method"}})
        continue
    if "id" in frame:
        send({"jsonrpc": "2.0", "id": frame["id"], "result": result})
