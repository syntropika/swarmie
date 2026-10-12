#!/usr/bin/env python3
"""Everyday interaction QA on an isolated real Native SDK system WebView."""
import argparse
import importlib.util
import json
from pathlib import Path
import sqlite3
import subprocess
import sys
import time

fixture = Path(__file__).resolve()
spec = importlib.util.spec_from_file_location("desktop_fixture", fixture.parents[1] / "desktop-gui/acceptance.py")
native = importlib.util.module_from_spec(spec)
spec.loader.exec_module(native)


class Everyday(native.Desktop):
    def __init__(self, root, artifacts):
        super().__init__(root, artifacts)
        (self.project / "attachment.txt").write_text("ATTACHMENT_DRAFT_MARKER\n", encoding="utf-8")
        (self.project / "review.txt").write_text("Review baseline line.\nKeep line.\n", encoding="utf-8")
        for args in [("init", "-b", "main"), ("config", "user.name", "Fixture"),
                     ("config", "user.email", "fixture@example.invalid"), ("add", "."),
                     ("commit", "-m", "Initialize disposable everyday GUI fixture")]:
            subprocess.run(["git", "-C", str(self.project), *args], env=self.env,
                           capture_output=True, check=True)
        (self.project / "review.txt").write_text("Review changed marker.\nKeep line.\n", encoding="utf-8")

    def child(self, command, **kwargs):
        if "--mock-script" in command:
            path = Path(command[command.index("--mock-script") + 1])
            script = json.loads(path.read_text())
            first = script["turns"][0]
            normal = [{"steps": [{"type": "text", "itemId": "ack-" + str(index),
                                    "text": "Everyday fixture prompt " + str(index) + " received."},
                                   {"type": "end", "outcome": "completed"}]} for index in range(1, 13)]
            permission = {"id": "everyday-permission", "kind": "permission",
                "title": "Fixture requires a harmless permission", "description": "Allow the controlled fixture to complete.",
                "paths": [], "options": [{"id": "allow", "label": "Allow fixture", "kind": "allow"},
                                           {"id": "deny", "label": "Deny fixture", "kind": "deny"}], "metadata": {}}
            script["turns"] = [first, *normal, {"steps": [
                {"type": "sleep", "milliseconds": 3000}, {"type": "request", "request": permission},
                {"type": "await_answer", "id": permission["id"], "option": "allow"},
                {"type": "text", "itemId": "permission-finished", "text": "Permission fixture completed."},
                {"type": "end", "outcome": "completed"}]}]
            path.write_text(json.dumps(script, indent=2) + "\n", encoding="utf-8")
        return super().child(command, **kwargs)

    def launch_host(self):
        self.host = self.child(["dbus-run-session", "--config-file=" + str(self.artifacts / "dbus.conf"),
                               "--", str(self.root / "apps/desktop/zig-out/bin/swarmie-desktop")],
                              log="host.log", cwd=self.root / "apps/desktop")
        deadline = time.monotonic() + 20
        while time.monotonic() < deadline:
            result = subprocess.run(["xdotool", "search", "--onlyvisible", "--name", "^swarmie$"],
                                    env=self.env, capture_output=True, text=True)
            for window in reversed(result.stdout.splitlines()):
                pid = subprocess.run(["xdotool", "getwindowpid", window], env=self.env,
                                     capture_output=True, text=True)
                if pid.returncode:
                    continue
                group = subprocess.run(["ps", "-o", "pgid=", "-p", pid.stdout.strip()],
                                       capture_output=True, text=True).stdout.strip()
                # A closing old GTK window can briefly remain visible during restart.
                if group != str(self.host.pid):
                    continue
                self.window = window
                self.x("windowsize", self.window, "1280", "900")
                self.x("windowfocus", "--sync", self.window)
                time.sleep(3)
                return
            if self.host.poll() is not None:
                raise RuntimeError("Owned native host exited; inspect host.log")
            time.sleep(0.1)
        raise RuntimeError("The current owned native host window was not visible")

    def profile(self, mutate=None):
        paths = list((self.artifacts / "xdg-data").rglob("localstorage.sqlite3"))
        if len(paths) != 1:
            raise RuntimeError("Expected exactly one owned native local-storage database")
        with sqlite3.connect(str(paths[0])) as database:
            row = database.execute("SELECT value FROM ItemTable WHERE key = ?", ("swarmie.workspace.v2",)).fetchone()
            if not row:
                raise RuntimeError("The actual GUI has not persisted its workspace")
            raw = row[0]
            text = raw.decode("utf-16-le") if isinstance(raw, bytes) else raw
            value = json.loads(text)
            if mutate:
                mutate(value)
                database.execute("UPDATE ItemTable SET value = ? WHERE key = ?",
                                 (json.dumps(value).encode("utf-16-le"), "swarmie.workspace.v2"))
            return value

    def action(self, request):
        operation = request["action"]
        if operation == "inspect-state":
            result = self.profile()
            (self.artifacts / (request.get("name", "workspace-state") + ".json")).write_text(
                json.dumps(result, indent=2) + "\n", encoding="utf-8")
            return result
        if operation == "assert-prompt":
            session = request.get("sessionId", self.session)
            events = [json.loads(line) for line in (self.artifacts / "sessions" / (session + ".jsonl")).read_text().splitlines()]
            users = [event for event in events if event.get("type") == "message.user"]
            actual = users[-1]
            text = "\n".join(part["data"] for part in actual["data"]["content"] if part["type"] == "text")
            for marker in request.get("contains", []):
                assert marker in text, "Submitted prompt lacks: " + marker
            for marker in request.get("excludes", []):
                assert marker not in text, "Submitted prompt unexpectedly contains: " + marker
            if "equals" in request:
                assert text == request["equals"], "Submitted prompt differs from the exact expected text"
            result = {"actualUserEvent": actual, "passed": True}
            (self.artifacts / (request["name"] + ".json")).write_text(json.dumps(result, indent=2) + "\n", encoding="utf-8")
            return result
        if operation in ("seed-timers", "seed-age-boundaries"):
            self.stop(self.host)
            now = round(time.time() * 1000)
            def mutate(value):
                if operation == "seed-timers":
                    assert len(value["threads"]) >= 2, "Create two GUI conversations before seeding timers"
                    for thread, delay in zip(value["threads"][:2], [8000, 16000]):
                        thread["snoozeUntil"] = now + delay
                        thread["settled"] = False
                else:
                    threads = [thread for thread in value["threads"] if not thread.get("sessionId")]
                    assert len(threads) >= 2, "Create two unsent GUI conversations before seeding age boundaries"
                    for thread, label, age, delay in zip(threads[:2],
                            ["Week boundary fixture", "Day boundary fixture"],
                            [7 * 86400000, 86400000], [24000, 15000]):
                        thread.update(title=label, createdAtMs=now - age + delay,
                                      updatedAtMs=now - age + delay, settled=False)
                        thread.setdefault("layout", {})["panel"] = None
                        thread.pop("snoozeUntil", None)
            result = self.profile(mutate)
            (self.artifacts / (operation + ".json")).write_text(json.dumps({"seededAtMs": now, "workspace": result}, indent=2) + "\n", encoding="utf-8")
            self.launch_host()
            delays = [8000, 16000] if operation == "seed-timers" else [15000, 24000]
            return {"seededAtMs": now, "boundariesMs": [now + delay for delay in delays]}
        if operation == "type":
            self.x("windowfocus", "--sync", self.window)
            for index, line in enumerate(request["text"].split("\n")):
                if index:
                    self.x("key", "--clearmodifiers", "Return")
                if line:
                    self.x("type", "--clearmodifiers", "--delay", "50", "--", line)
            time.sleep(0.5)
            return {"action": operation, "completed": True}
        return super().action(request)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=fixture.parents[3])
    parser.add_argument("--artifacts", type=Path, required=True)
    args = parser.parse_args()
    root, artifacts = args.root.resolve(), args.artifacts.resolve()
    if artifacts == root or root in artifacts.parents:
        parser.error("Artifacts must be outside the repository")
    desktop = Everyday(root, artifacts)
    try:
        print(json.dumps({"ready": desktop.start()}), flush=True)
        for line in sys.stdin:
            if not line.strip():
                continue
            request = json.loads(line)
            try:
                response = {"request": request, "result": desktop.action(request)}
            except Exception as error:
                response = {"request": request, "error": str(error)}
            with (artifacts / "actions.jsonl").open("a", encoding="utf-8") as output:
                output.write(json.dumps(response) + "\n")
            print(json.dumps(response), flush=True)
    finally:
        desktop.close()


if __name__ == "__main__":
    main()
