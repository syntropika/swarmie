#!/usr/bin/env python3
"""Deterministic protocol peer used by integration tests, without model access."""
import json
import os
import sys
import threading
import time

mode = sys.argv[1]
lock = threading.Lock()
turn = 0
permission = None
active = False
pending_question = False
selected_model = "fixture-model"
thinking = "medium"
session = "fixture-session"


def emit(frame):
    with lock:
        print(json.dumps(frame, ensure_ascii=False), flush=True)


def notify(method, params):
    emit({"jsonrpc": "2.0", "method": method, "params": params})


def response(request, value=None):
    if mode == "pi":
        emit({"id": request["id"], "type": "response", "command": request["type"], "success": True, "data": value})
    elif mode == "claude":
        emit({"type": "control_response", "response": {"subtype": "success", "request_id": request["request_id"], "response": value}})
    else:
        emit({"jsonrpc": "2.0", "id": request["id"], "result": value})


def text(value):
    if mode == "codex":
        notify("item/agentMessage/delta", {"threadId": session, "itemId": "message", "delta": value})
    elif mode == "pi":
        emit({"type": "message_update", "assistantMessageEvent": {"type": "text_delta", "delta": value}})
    elif mode == "claude":
        emit({"type": "stream_event", "event": {"type": "content_block_delta", "delta": {"type": "text_delta", "text": value}}})
    else:
        notify("session/update", {"sessionId": session, "update": {"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": value}}})


def finish(request=None, reason="completed"):
    global active
    active = False
    if mode == "codex":
        notify("turn/completed", {"threadId": session, "turn": {"id": f"turn-{turn}", "status": reason}})
    elif mode == "pi":
        emit({"type": "agent_end", "messages": []})
        emit({"type": "agent_settled"})
    elif mode == "claude":
        emit({"type": "result", "is_error": reason == "failed", "usage": {"input_tokens": 3, "output_tokens": 2}})
    else:
        response(request, {"stopReason": "cancelled" if reason == "cancelled" else "end_turn"})


def prompt(request, value):
    global active, turn, permission, pending_question
    turn += 1
    active = True
    if value == "crash":
        os._exit(7)
    if mode == "codex":
        response(request, {"turn": {"id": f"turn-{turn}", "status": "inProgress"}})
        notify("turn/started", {"threadId": session, "turn": {"id": f"turn-{turn}"}})
        notify("turn/completed", {"threadId": "child-session", "turn": {"id": "child-turn", "status": "completed"}})
    elif mode == "pi":
        response(request, {"disposition": "handled" if value == "handled" else "started"})
        if value == "handled":
            active = False
            return
        emit({"type": "agent_start"})
    if value == "permission" and mode != "pi":
        permission = request
        if mode == "codex":
            emit({"jsonrpc": "2.0", "id": 500, "method": "item/commandExecution/requestApproval", "params": {"threadId": session, "command": "echo fixture", "reason": "Test approval"}})
        elif mode == "claude":
            emit({"type": "control_request", "request_id": "approval-1", "request": {"subtype": "can_use_tool", "tool_name": "Bash", "input": {"command": "echo fixture"}}})
        else:
            emit({"jsonrpc": "2.0", "id": 500, "method": "session/request_permission", "params": {"sessionId": session, "toolCall": {"title": "Test approval"}, "options": [{"optionId": "allow_once", "name": "Allow", "kind": "allow_once"}, {"optionId": "reject_once", "name": "Reject", "kind": "reject_once"}, {"optionId": "allow_always", "name": "Allow for session", "kind": "allow_always"}, {"optionId": "opaque", "name": "Custom choice", "kind": "provider_custom"}]}})
        return
    if value == "question" and mode in ("codex", "claude", "pi"):
        permission = request
        pending_question = True
        questions = [{"id": "target", "header": "Target", "question": "Which target?", "options": [{"label": "Linux"}, {"label": "macOS"}], "isOther": True}]
        if mode == "codex":
            emit({"jsonrpc": "2.0", "id": 500, "method": "item/tool/requestUserInput", "params": {"threadId": session, "questions": questions}})
        elif mode == "claude":
            questions[0].pop("id")
            emit({"type": "control_request", "request_id": "question-1", "request": {"subtype": "can_use_tool", "tool_name": "AskUserQuestion", "input": {"questions": questions}}})
        else:
            emit({"type": "extension_ui_request", "id": "dialog-1", "method": "select", "title": "Which target?", "options": ["Linux", "macOS"]})
        return
    if value == "configuration" and mode == "codex":
        text(json.dumps(request["params"]))
        finish(request)
        return
    if value == "hold":
        if mode == "acp":
            permission = request
        return
    if value in ("flood", "burst"):
        for _ in range(800 if value == "burst" else 8000):
            text("flood")
        finish(request)
        return
    if mode == "claude" and value in ("claude-stream", "claude-tool-failed"):
        emit({"type": "stream_event", "uuid": "frame-start", "event": {"type": "message_start", "message": {"id": "message-stable"}}})
        for index, chunk in enumerate(["A table:\n", "| File | Contents |\n|---|---|\n| hello.txt | fixture |"]):
            emit({"type": "stream_event", "uuid": f"frame-{index}", "event": {"type": "content_block_delta", "index": 0, "delta": {"type": "text_delta", "text": chunk}}})
        emit({"type": "stream_event", "event": {"type": "content_block_start", "index": 1, "content_block": {"type": "tool_use", "id": "read-stable", "name": "Read", "input": {}}}})
        for chunk in ['{"file_path":', '"hello.txt"}']:
            emit({"type": "stream_event", "event": {"type": "content_block_delta", "index": 1, "delta": {"type": "input_json_delta", "partial_json": chunk}}})
        emit({"type": "user", "message": {"content": [{"type": "tool_result", "tool_use_id": "read-stable", "content": "fixture", "is_error": value == "claude-tool-failed"}]}})
        finish(request)
        return
    text("Hello \u2028 runtime")
    if mode == "codex":
        notify("item/reasoning/textDelta", {"threadId": session, "delta": "reason"})
        notify("item/started", {"threadId": session, "item": {"id": "t1", "type": "commandExecution", "command": "echo fixture"}})
        notify("item/completed", {"threadId": session, "item": {"id": "t1", "type": "commandExecution", "exitCode": 0}})
        notify("thread/tokenUsage/updated", {"threadId": session, "tokenUsage": {"total": {"totalTokens": 5}}})
    elif mode == "pi":
        emit({"type":"message_end","message":{"role":"user","content":"hello"}})
        if value == "retry-error":
            emit({"type":"message_end","message":{"role":"assistant","stopReason":"error","errorMessage":"temporary model failure"}})
            emit({"type":"auto_retry_start","attempt":1})
            emit({"type":"message_end","message":{"role":"assistant","stopReason":"stop","content":[]}})
        emit({"type": "tool_execution_start", "toolCallId": "t1", "toolName": "read"})
        emit({"type": "tool_execution_end", "toolCallId": "t1", "result": {"text": "fixture"}})
        emit({"type": "agent_end", "messages": []})
        if value == "retry":
            emit({"type": "auto_retry_start", "attempt": 1})
            time.sleep(0.05)
            text("retry output")
    elif mode == "claude":
        emit({"type": "result", "parent_tool_use_id": "child-tool", "is_error": False})
    finish(request)


for line in sys.stdin:
    try:
        r = json.loads(line)
        method = r.get("method", r.get("type"))
        if mode == "claude":
            if r.get("type") == "control_request":
                method = r["request"]["subtype"]
            elif r.get("type") == "user":
                prompt(r, r["message"]["content"][0]["text"])
                continue
            elif r.get("type") == "control_response":
                text(json.dumps(r) if pending_question else "approved")
                pending_question = False
                finish(permission)
                continue
        if mode == "pi" and r.get("type") == "extension_ui_response":
            text(json.dumps(r))
            pending_question = False
            finish(permission)
            continue
        if "result" in r and r.get("id") == 500:
            text(json.dumps(r) if pending_question else "approved")
            pending_question = False
            finish(permission)
        elif method == "initialize":
            if mode == "acp":
                response(r, {"protocolVersion": 1, "agentCapabilities": {"loadSession": True, "promptCapabilities": {"image": True}}})
            elif mode == "claude":
                response(r, {"models": [{"value": "fixture-model", "supportedEffortLevels":["low","medium","high"], "supportsFastMode":True}]})
            else:
                response(r, {"userAgent": "fixture"})
        elif method in ("thread/start", "thread/resume", "thread/fork"):
            response(r, {"thread": {"id": session}, "model": "fixture-model", "approvalPolicy": "on-request"})
        elif method in ("session/new", "session/load"):
            response(r, {"sessionId": session})
        elif method == "get_state":
            response(r, {"sessionFile": "/tmp/fixture-pi-session.jsonl", "sessionId": session, "isStreaming": active, "model": {"id": selected_model,"provider":"fixture","name":"Fixture","reasoning":True,"contextWindow":32768},"thinkingLevel":thinking})
        elif method in ("turn/start", "prompt", "session/prompt"):
            if mode == "codex":
                value = r["params"]["input"][0]["text"]
            elif mode == "pi":
                value = r["message"]
            else:
                value = r["params"]["prompt"][0]["text"]
            if value == "reject":
                if mode == "pi":
                    emit({"id": r["id"], "type": "response", "command": "prompt", "success": False, "error": "rejected"})
                else:
                    emit({"jsonrpc": "2.0", "id": r["id"], "error": {"code": -32000, "message": "rejected"}})
            else:
                prompt(r, value)
        elif method in ("turn/steer", "steer"):
            response(r, {"disposition": "queued"})
            text("steered")
        elif method in ("turn/interrupt", "abort", "interrupt", "session/cancel"):
            if method != "session/cancel":
                response(r, {})
            if active:
                finish(permission, "cancelled")
        elif method == "clear_queue":
            response(r, {"steering": [], "followUp": []})
        elif method in ("model/list", "get_available_models"):
            response(r, {"models": [{"id": "fixture-model", "model":"fixture-model", "provider":"fixture", "name":"Fixture", "reasoning":True,"supportedReasoningEfforts":[{"reasoningEffort":"low"},{"reasoningEffort":"medium"},{"reasoningEffort":"high"}]}]})
        elif method == "set_thinking_level":
            thinking = r["level"]
            response(r, {})
        elif method == "get_available_thinking_levels":
            response(r, {"levels":["off","low","medium","high"]})
        elif method == "apply_flag_settings":
            settings = r["request"]["settings"]
            assert set(settings).issubset({"effortLevel", "fastMode"})
            if "effortLevel" in settings:
                assert settings["effortLevel"] in {"low","medium","high"}
            if "fastMode" in settings:
                assert isinstance(settings["fastMode"],bool)
            response(r, settings)
        elif method in ("thread/rollback", "thread/compact/start", "compact", "set_model", "set_thinking_level", "set_permission_mode"):
            response(r, {})
    except Exception as error:
        print(error, file=sys.stderr)
        sys.exit(2)
