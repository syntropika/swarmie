"""Native protocol peer for shared-origin invocation and overlay lifecycle tests."""
import json
import os
import pathlib
import sys

provider = sys.argv[1]
cwd = pathlib.Path.cwd()
home = pathlib.Path(os.environ["HOME"])
ledger = pathlib.Path(os.environ["SWARMIE_SKILL_LEDGER"])
args = sys.argv[2:]


def emit(frame):
    print(json.dumps(frame), flush=True)


def record(frame):
    with ledger.open("a") as output:
        output.write(json.dumps(frame) + "\n")


def flag(name):
    return args[args.index(name) + 1] if name in args else None


def entries():
    roots = [(cwd / ".agents/skills", "project"), (home / ".agents/skills", "user")]
    if provider == "claude":
        roots = [(pathlib.Path(flag("--add-dir")) / ".claude/skills", "project")] if flag("--add-dir") else []
    if "--no-skills" in args or "--disable-slash-commands" in args:
        roots = []
    skills = []
    for root, scope in roots:
        if not root.exists():
            continue
        for directory in sorted(root.iterdir()):
            path = directory / "SKILL.md"
            if not path.is_file():
                continue
            name = next((line[6:] for line in path.read_text().splitlines() if line.startswith("name: ")), directory.name)
            if provider == "pi" and any(skill["name"] == name for skill in skills):
                continue
            skills.append({"name": name, "path": str(path.resolve()), "scope": scope, "enabled": True, "description": "Read a supporting reference", "alias": directory.name})
    return skills


def catalog():
    if provider == "claude":
        return [{"name": skill["name"], "aliases": [skill["alias"]], "description": skill["description"]} for skill in entries()]
    if provider == "pi":
        return [{"name": "skill:" + skill["name"], "source": "skill", "description": skill["description"], "sourceInfo": {"path": skill["path"], "scope": skill["scope"]}} for skill in entries()]
    if os.environ.get("SWARMIE_NATIVE_SKILL_ALIASES"):
        return [{"name": "same-name", "path": path, "enabled": True} for path in json.loads(os.environ["SWARMIE_NATIVE_SKILL_ALIASES"])]
    return [{key: value for key, value in skill.items() if key != "alias"} for skill in entries()]


def invocation(request):
    if provider == "codex":
        selected = next(item for item in request["params"]["input"] if item["type"] == "skill")
        path = pathlib.Path(selected["path"])
    else:
        text = request["message"]["content"][-1]["text"] if provider == "claude" else request["message"]
        name = text.split()[0][1:]
        name = name.removeprefix("skill:")
        selected = next(skill for skill in entries() if skill["alias"] == name or skill["name"] == name)
        path = pathlib.Path(selected["path"])
    reference = (path.parent / "references/detail.txt").read_text()
    record({"request": request, "selectedPath": str(path), "reference": reference})


record({"launchArgs": args, "configHome": os.environ.get("CLAUDE_CONFIG_DIR"), "cwd": str(cwd)})
for line in sys.stdin:
    request = json.loads(line)
    if provider == "claude":
        if request["type"] == "control_request":
            operation = request["request"]["subtype"]
            response = {"commands": catalog(), "pid": os.getpid()} if operation == "initialize" else {}
            frame = {"type": "control_response", "response": {"subtype": "success", "request_id": request["request_id"], "response": response}}
            if os.environ.get("SWARMIE_FAIL_INIT") and operation == "initialize":
                frame["response"] = {"subtype": "error", "request_id": request["request_id"], "error": "Controlled init failure"}
            emit(frame)
        elif request["type"] == "user":
            invocation(request)
            emit({"type": "result", "is_error": False, "result": "done"})
    elif provider == "pi":
        operation = request["type"]
        data = {"sessionId": "pi-session"} if operation == "get_state" else {"commands": catalog()} if operation == "get_commands" else {}
        if operation == "prompt":
            invocation(request)
        emit({"type": "response", "id": request["id"], "command": operation, "success": True, "data": data})
        if operation == "prompt":
            emit({"type": "agent_settled", "reason": "completed"})
    else:
        operation = request.get("method")
        if "id" not in request:
            continue
        params = request.get("params", {})
        response = {}
        if operation in ("thread/start", "thread/resume", "thread/fork"):
            response = {"thread": {"id": "codex-thread"}}
        elif operation == "skills/list":
            response = {"data": [{"cwd": str(cwd), "skills": catalog(), "errors": []}]}
        elif operation == "turn/start":
            invocation(request)
            response = {"turn": {"id": "turn"}}
        emit({"jsonrpc": "2.0", "id": request["id"], "result": response})
        if operation == "turn/start":
            emit({"jsonrpc": "2.0", "method": "turn/completed", "params": {"threadId": "codex-thread", "turn": {"id": "turn", "status": "completed"}}})
