"""Native-shaped Claude lifecycle peer with persistent fork/cut semantics."""
import json
import os
import pathlib
import sys
import time
import uuid

args = sys.argv[1:]
root = pathlib.Path.cwd()
sessions = root / "native-sessions"
sessions.mkdir(exist_ok=True)


def flag(name):
    return args[args.index(name) + 1] if name in args else None


session = flag("--session-id") or flag("--resume")
history = []
failure = None
if "--resume" in args:
    try:
        history = json.loads((sessions / (flag("--resume") + ".json")).read_text())
    except Exception:
        failure = "Native resume transcript is missing"
if "--fork-session" in args:
    if not flag("--session-id"):
        failure = "Fork must name its actual new session ID"
    if (root / "fail-fork").exists():
        failure = "Native fork initialization failed"
    anchor = next((a.split("=", 1)[1] for a in args if a.startswith("--resume-session-at=")), None)
    if anchor:
        matches = [i for i, turn in enumerate(history) if turn["assistant"] == anchor]
        if matches:
            history = history[:matches[0] + 1]
        else:
            failure = "Native fork cut is not an assistant transcript UUID"
model = flag("--model") or "fixture-model"
settings = {}
queued = None
pending_permission = None
pending_active = None


def emit(value):
    print(json.dumps(value), flush=True)


def receipt(request, value=None, error=None):
    response = {"subtype": "error" if error else "success", "request_id": request["request_id"]}
    response["error" if error else "response"] = error if error else value
    emit({"type": "control_response", "response": response})


def save():
    (sessions / (session + ".json")).write_text(json.dumps(history))


for line in sys.stdin:
    request = json.loads(line)
    if request["type"] == "control_response":
        answer = request["response"]["response"]
        (root / "permission-answer.json").write_text(json.dumps(answer))
        if pending_permission:
            time.sleep(0.15)
            emit({"type": "result", "is_error": answer.get("interrupt", False), "result": "Permission turn interrupted" if answer.get("interrupt") else "Permission handled"})
            pending_permission = None
        continue
    if request["type"] == "control_request":
        control = request["request"]
        operation = control["subtype"]
        if operation == "initialize":
            if failure:
                receipt(request, error=failure)
                continue
            save()
            receipt(request, {"models": [{"value": "fixture-model", "displayName": "Fixture", "supportedEffortLevels": ["low", "high"], "supportsFastMode": True}], "commands": [{"name": "compact"}]})
        elif operation == "get_usage":
            if (root / "quota-none").exists():
                receipt(request, {"subscription_type": None, "rate_limits": {}})
            elif (root / "quota-rich").exists():
                receipt(request, {"subscription_type": "max", "rate_limits": {"limits": [{"id": "session", "group": "session", "percent": 42.5, "resets_at": "2026-10-04T01:02:03+00:00"}, {"kind": "weekly", "percent": 125, "resets_at": "invalid"}], "cedar_ember": {"eligible": True, "next_grant_id": "live", "grants": [{"id": "live", "paused": False, "usable_now": True, "ends_at": "2099-10-04T01:02:03Z", "resets_left": 2}, {"id": "expired", "usable_now": True, "ends_at": "2000-01-01T00:00:00Z", "resets_left": 8}]}}})
            else:
                receipt(request, {"subscription_type": "pro", "rate_limits": {"five_hour": {"utilization": 20, "resets_at": "2026-10-04T01:02:03Z"}, "seven_day": {"utilization": 5, "resets_at": None}}})
        elif operation == "rewind_files":
            with (root / "rewind.jsonl").open("a") as output:
                output.write(json.dumps(control) + "\n")
            denied = (root / "deny-rewind").exists() or (not control["dry_run"] and (root / "deny-actual-rewind").exists())
            if denied:
                receipt(request, {"canRewind": False, "error": "Native file rewind refused"})
            else:
                if not control["dry_run"]:
                    (root / "agent-file.txt").write_text("restored by native rewind")
                receipt(request, {"canRewind": True})
        elif operation == "set_model":
            model = control["model"]
            receipt(request, {})
        elif operation == "apply_flag_settings":
            settings.update(control["settings"])
            receipt(request, {})
        elif operation == "interrupt":
            if pending_active is not None:
                emit({"type": "result", "subtype": "error_during_execution" if pending_active else "success", "is_error": pending_active})
                pending_active = None
            receipt(request, {"cancelled": [queued] if queued else []})
            queued = None
        else:
            receipt(request, {})
        continue
    if request["type"] != "user":
        continue
    emit({"type": "system", "subtype": "init", "session_id": session, "model": model})
    content = request["message"]["content"]
    text = content if isinstance(content, str) else "".join(part.get("text", "") for part in content)
    if text in ("ask-permission", "ask-plan", "ask-tool"):
        pending_permission = request["uuid"]
        tool = {"ask-permission": "AskUserQuestion", "ask-plan": "ExitPlanMode", "ask-tool": "Bash"}[text]
        payload = {"questions": [{"question": "Proceed?", "options": [{"label": "Yes"}]}]} if text == "ask-permission" else {"plan": "Implement the fixture plan."} if text == "ask-plan" else {"command": "echo fixture"}
        emit({"type": "control_request", "request_id": "permission-fixture", "request": {"subtype": "can_use_tool", "tool_name": tool, "input": payload}})
        continue
    if text == "commands-update":
        emit({"type": "system", "subtype": "commands_changed", "commands": [{"name": "fixture-command", "description": "Updated native command"}]})
    if text == "background-tools":
        emit({"type": "stream_event", "event": {"type": "content_block_start", "index": 0, "content_block": {"type": "tool_use", "id": "background-bash", "name": "Bash", "input": {"command": "sleep 1"}}}})
        emit({"type": "tool_progress", "parent_tool_use_id": "background-bash", "tool_use_id": "bash-progress-0", "elapsed_time_seconds": 1.25})
        emit({"type": "user", "message": {"content": [{"type": "tool_result", "tool_use_id": "background-bash", "content": "Started background command"}]}, "tool_use_result": {"backgroundTaskId": "native-task"}})
    if text == "denied-tool":
        emit({"type": "stream_event", "event": {"type": "content_block_start", "index": 0, "content_block": {"type": "tool_use", "id": "denied-bash", "name": "Bash", "input": {"command": "forbidden"}}}})
        emit({"type": "system", "subtype": "permission_denied", "tool_use_id": "denied-bash", "message": "Native rule refused tool"})
        emit({"type": "user", "message": {"content": [{"type": "tool_result", "tool_use_id": "denied-bash", "content": "Denied", "is_error": True}]}})
    if text in ("active-native-cancel", "successful-cancel-race"):
        pending_active = text == "active-native-cancel"
        continue
    if text == "queued-native-cancel":
        queued = request["uuid"]
        continue
    if text == "/compact":
        emit({"type": "system", "subtype": "status", "status": "compacting"})
        emit({"type": "result", "parent_tool_use_id": "compact-child", "is_error": False})
        time.sleep(0.2)
        refused = (root / "refuse-compact").exists()
        if refused:
            emit({"type": "system", "subtype": "status", "compact_error": "Not enough messages to compact"})
        else:
            history.clear()
            save()
            emit({"type": "system", "subtype": "compact_boundary"})
        emit({"type": "result", "is_error": False})
        continue
    user = request["uuid"]
    uuid.UUID(user)
    assistant = str(uuid.uuid4())
    before = [turn["text"] for turn in history]
    history.append({"user": user, "assistant": assistant, "text": text})
    save()
    (root / "agent-file.txt").write_text("changed by " + text)
    body = json.dumps({"session": session, "history": before, "text": text, "args": args, "settings": settings, "model": model, "checkpointing": os.environ.get("CLAUDE_CODE_ENABLE_SDK_FILE_CHECKPOINTING")})
    message = {"type": "assistant", "message": {"id": "main-message", "role": "assistant", "content": [{"type": "text", "text": body}]}}
    if not (root / "omit-main-uuid").exists():
        message["uuid"] = assistant
    emit(message)
    emit({"type": "assistant", "uuid": str(uuid.uuid4()), "parent_tool_use_id": "child-task", "message": {"role": "assistant", "content": [{"type": "text", "text": "child output"}]}})
    emit({"type": "result", "parent_tool_use_id": "child-task", "is_error": False})
    emit({"type": "result", "is_error": False, "usage": {"input_tokens": 10, "output_tokens": 2}})
    if text == "background-tools":
        time.sleep(0.1)
        emit({"type": "system", "subtype": "task_progress", "tool_use_id": "background-bash", "task_id": "native-task", "summary": "Still running", "usage": {"total_tokens": 25}})
        emit({"type": "system", "subtype": "task_notification", "tool_use_id": "background-bash", "task_id": "native-task", "status": "failed", "summary": "Native background process failed"})
