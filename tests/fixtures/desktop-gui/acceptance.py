#!/usr/bin/env python3
"""Disposable native WebView QA; all UI input and capture use the X11 OS surface."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import select
import signal
import shutil
import subprocess
import sys
import time
import urllib.request


class Desktop:
    def __init__(self, root, artifacts, output_fixtures=None):
        self.root, self.artifacts = root, artifacts
        self.children, self.logs, self.receipts = [], [], []
        self.env = {key: value for key, value in os.environ.items() if key != "MPAI_TOKEN"}
        artifacts.mkdir(parents=True, exist_ok=True)
        home = artifacts / "home"
        home.mkdir(exist_ok=True)
        config = artifacts / "xdg-config"
        config.mkdir(exist_ok=True)
        downloads = artifacts / "downloads"
        downloads.mkdir(exist_ok=True)
        (config / "user-dirs.dirs").write_text('XDG_DOWNLOAD_DIR="' + str(downloads) + '"\n', encoding="utf-8")
        self.env.update(HOME=str(home), XDG_CONFIG_HOME=str(config))
        self.project = artifacts / "project"
        self.project.mkdir(exist_ok=True)
        (self.project / "hello.txt").write_text("First fixture line.\nSecond fixture line.\nThird fixture line.\n", encoding="utf-8")
        (self.project / "second.txt").write_text("Second file baseline.\n", encoding="utf-8")
        skill = self.project / ".agents/skills/acceptance/SKILL.md"
        skill.parent.mkdir(parents=True, exist_ok=True)
        skill.write_text("---\nname: acceptance\ndescription: Deterministic acceptance resource\n---\nRead the project fixture and report its contents.\n", encoding="utf-8")
        (self.project / "index.html").write_text('<!doctype html><html><body style="background:#102030;color:white;font:24px sans-serif"><h1>Native preview fixture</h1><button onclick="document.getElementById(\'result\').textContent=\'JavaScript interaction confirmed\'">Run JavaScript</button><p id="result">Waiting for interaction</p></body></html>', encoding="utf-8")
        self.outputs = []
        if output_fixtures:
            for source in sorted(output_fixtures.glob("fixture.*")):
                if source.is_file():
                    target = self.project / source.name
                    shutil.copyfile(source, target)
                    self.outputs.append(target)
        # These peers shadow only this daemon's PATH, never the user's installed tools.
        peers = artifacts / "bin"
        peers.mkdir(exist_ok=True)
        for name, script in [("codex", "native_peer.py"), ("gh", "gh_peer.py")]:
            executable = peers / name
            executable.write_text("#!/bin/sh\nexec python3 " + str(Path(__file__).with_name(script)) + ' "$@"\n', encoding="utf-8")
            executable.chmod(0o755)
        self.env.update(PATH=str(peers) + os.pathsep + self.env["PATH"], MPAI_GUI_PEER_LEDGER=str(artifacts / "peer.jsonl"))

    def child(self, command, log=None, env=None, cwd=None, pipe=False):
        target = subprocess.PIPE if pipe else (self.artifacts / (log or "child.log")).open("a", encoding="utf-8")
        if not pipe:
            self.logs.append(target)
        process = subprocess.Popen(command, stdin=subprocess.DEVNULL, stdout=target, stderr=subprocess.STDOUT, text=True,
                                   env=env or self.env, cwd=cwd, start_new_session=True)
        self.children.append(process)
        return process

    def ready(self, process, prefix, timeout=25):
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            if process.poll() is not None:
                raise RuntimeError("Owned child exited before readiness")
            if select.select([process.stdout], [], [], 0.2)[0]:
                line = process.stdout.readline()
                with (self.artifacts / "readiness.log").open("a", encoding="utf-8") as output:
                    output.write(line)
                if line.startswith(prefix) and (prefix or line.strip().isdigit()):
                    return line[len(prefix):].strip()
        raise RuntimeError("Owned child readiness timed out")

    def rpc(self, method, params=None):
        request = urllib.request.Request(self.url + "/rpc", json.dumps({"jsonrpc": "2.0", "id": 1, "method": method, "params": params or {}}).encode(), {"Content-Type": "application/json"})
        with urllib.request.urlopen(request, timeout=20) as response:
            result = json.load(response)
        self.receipts.append({"method": method, "params": params or {}, "response": result})
        self.persist()
        if "error" in result:
            raise RuntimeError(json.dumps(result["error"]))
        return result["result"]

    def persist(self):
        (self.artifacts / "rpc.json").write_text(json.dumps(self.receipts, indent=2) + "\n", encoding="utf-8")

    def start(self):
        assets = self.artifacts / "desktop-assets"
        shutil.copytree(self.root / "apps/desktop/dist", assets, dirs_exist_ok=True)
        fingerprints = {str(path.relative_to(self.root)): hashlib.sha256(path.read_bytes()).hexdigest() for path in [self.root / "target/debug/mpai", self.root / "apps/desktop/zig-out/bin/mpai-desktop", *sorted((self.root / "apps/desktop/dist").rglob("*.js"))]}
        (self.artifacts / "build.json").write_text(json.dumps(fingerprints, indent=2) + "\n", encoding="utf-8")
        script = self.artifacts / "mock.json"
        steps = [{"type": "text", "itemId": "fixture-message", "text": "Native desktop acceptance fixture.\n\nOpen [hello.txt lines 2–3](hello.txt:2-3) or [second.txt](second.txt:1).\n\n```sh\nprintf 'terminal context fixture'\n```"}]
        steps += [{"type": "emit", "kind": {"type": "file.changed", "data": {"path": str(path)}}} for path in self.outputs]
        steps.append({"type": "end", "reason": "completed"})
        script.write_text(json.dumps({"turns": [{"steps": steps}, {"steps": [{"type": "text", "itemId": "fixture-context-ack", "text": "Fixture context received."}, {"type": "end", "reason": "completed"}]}]}), encoding="utf-8")
        daemon = self.child([str(self.root / "target/debug/mpai"), "--state-dir", str(self.artifacts / "sessions"), "serve", "--http", "127.0.0.1:0", "--desktop-assets", str(assets), "--workbench", "--git-worktrees", str(self.artifacts / "worktrees"), "--resources-root", str(self.artifacts / "resources"), "--mock-script", str(script)], pipe=True)
        self.url = self.ready(daemon, "HTTP listening on ").removesuffix("/rpc")
        created = self.rpc("session/create", {"harness": "mock", "cwd": str(self.project), "config": {}})
        self.session = created["id"]
        self.rpc("session/metadata", {"sessionId": self.session, "title": "Native acceptance fixture"})
        self.rpc("extension/invoke", {"extensionId": "resources", "command": "templates.put", "arguments": {"id": "fixture-review", "name": "Fixture review", "text": "Review {{file}} carefully.", "aliases": ["inspect-fixture"]}})
        self.rpc("session/prompt", {"sessionId": self.session, "text": "Show the fixture file links."})
        deadline = time.monotonic() + 10
        while self.rpc("session/get", {"sessionId": self.session})["state"] != "idle":
            if time.monotonic() > deadline:
                raise RuntimeError("Mock prompt did not complete")
            time.sleep(0.05)
        xvfb = self.child(["Xvfb", "-displayfd", "1", "-screen", "0", "1600x1000x24", "-nolisten", "tcp"], pipe=True)
        self.display = ":" + self.ready(xvfb, "")
        self.env.update(DISPLAY=self.display, MPAI_DESKTOP_URL=self.url + "/?session=" + self.session,
                        MPAI_DESKTOP_MANAGED="1", XDG_DATA_HOME=str(self.artifacts / "xdg-data"),
                        XDG_CACHE_HOME=str(self.artifacts / "xdg-cache"))
        (self.artifacts / "dbus.conf").write_text('<!DOCTYPE busconfig PUBLIC "-//freedesktop//DTD D-Bus Bus Configuration 1.0//EN" "http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd"><busconfig><type>session</type><listen>unix:tmpdir=/tmp</listen><policy context="default"><allow send_destination="*"/><allow receive_sender="*"/><allow own="*"/></policy></busconfig>', encoding="utf-8")
        self.launch_host()
        return {"url": self.url, "sessionId": self.session, "display": self.display, "window": self.window, "artifacts": str(self.artifacts)}

    def launch_host(self):
        self.host = self.child(["dbus-run-session", "--config-file=" + str(self.artifacts / "dbus.conf"), "--", str(self.root / "apps/desktop/zig-out/bin/mpai-desktop")], log="host.log", cwd=self.root / "apps/desktop")
        deadline = time.monotonic() + 20
        while time.monotonic() < deadline:
            result = subprocess.run(["xdotool", "search", "--onlyvisible", "--name", "^mpai$"], env=self.env, capture_output=True, text=True)
            if result.returncode == 0:
                self.window = result.stdout.splitlines()[-1]
                self.x("windowsize", self.window, "1280", "900")
                self.x("windowfocus", "--sync", self.window)
                time.sleep(3)
                return
            if self.host.poll() is not None:
                raise RuntimeError("Native host exited; inspect host.log")
            time.sleep(0.1)
        raise RuntimeError("Native window was not visible")

    def x(self, *args):
        return subprocess.run(["xdotool", *map(str, args)], stdin=subprocess.DEVNULL, env=self.env, check=True, capture_output=True, text=True).stdout

    def capture(self, name):
        if not name.replace("-", "").replace("_", "").isalnum():
            raise ValueError("Use a simple capture name")
        time.sleep(0.4)
        geometry = dict(line.split("=", 1) for line in self.x("getwindowgeometry", "--shell", self.window).splitlines() if "=" in line)
        target = self.artifacts / (name + ".png")
        subprocess.run(["ffmpeg", "-nostdin", "-loglevel", "error", "-y", "-f", "x11grab", "-video_size", geometry["WIDTH"] + "x" + geometry["HEIGHT"], "-i", self.display + "+" + geometry["X"] + "," + geometry["Y"], "-frames:v", "1", str(target)], stdin=subprocess.DEVNULL, env=self.env, check=True)
        ocr = subprocess.run(["tesseract", str(target), "stdout"], capture_output=True, text=True, check=True).stdout if shutil.which("tesseract") else ""
        (self.artifacts / (name + ".txt")).write_text(ocr, encoding="utf-8")
        return {"screenshot": str(target), "geometry": geometry, "ocr": ocr}

    def action(self, request):
        operation = request["action"]
        if operation == "capture":
            result = self.capture(request["name"])
            if request.get("contains") and not shutil.which("tesseract"):
                raise RuntimeError("OCR assertions require tesseract; screenshot saved for visual inspection")
            for expected in request.get("contains", []):
                if expected not in result["ocr"]:
                    raise AssertionError("Screenshot OCR did not contain: " + expected)
            return result
        if operation == "click":
            self.x("mousemove", "--window", self.window, request["x"], request["y"], "click", request.get("button", 1))
        elif operation == "key":
            self.x("windowfocus", "--sync", self.window)
            keys = request["key"] if isinstance(request["key"], list) else [request["key"]]
            self.x("key", "--clearmodifiers", *keys)
        elif operation == "type":
            self.x("windowfocus", "--sync", self.window)
            self.x("type", "--clearmodifiers", "--delay", "20", "--", request["text"])
        elif operation == "resize":
            self.x("windowsize", self.window, request["width"], request["height"])
        elif operation == "drag":
            self.x("mousemove", "--window", self.window, request["x"], request["y"], "mousedown", "1")
            time.sleep(0.2)
            for step in range(1, 5):
                self.x("mousemove", "--sync", "--window", self.window,
                       round(request["x"] + (request["toX"] - request["x"]) * step / 4),
                       round(request["y"] + (request["toY"] - request["y"]) * step / 4))
                time.sleep(0.15)
            self.x("mouseup", "1")
        elif operation == "rpc":
            return self.rpc(request["method"], request.get("params"))
        elif operation in ("clipboard-seed", "clipboard-read"):
            clipboard_env = {**self.env, "MPAI_GUI_PRIVATE_DISPLAY": self.display, "GTK_A11Y": "none"}
            command = ["/usr/bin/python3", str(Path(__file__).with_name("clipboard.py"))]
            if operation == "clipboard-seed":
                owner = self.child(command + ["seed", request["text"]], env=clipboard_env, pipe=True)
                return {"seeded": self.ready(owner, "CLIPBOARD_SEEDED="), "display": self.display, "ownerPid": owner.pid}
            output = subprocess.run(command + ["read"], stdin=subprocess.DEVNULL, capture_output=True, text=True, env=clipboard_env, timeout=8, check=True)
            result = json.loads(output.stdout)
            if "equals" in request and result.get("clipboardText") != request["equals"]:
                raise AssertionError("The private OS clipboard did not match the expected literal: " + json.dumps(result))
            return result
        elif operation == "restart-host":
            self.stop(self.host)
            self.launch_host()
        elif operation == "wait":
            time.sleep(min(10, float(request.get("seconds", 0.4))))
        else:
            raise ValueError("Unknown native action: " + operation)
        time.sleep(0.3)
        return {"action": operation, "completed": True}

    @staticmethod
    def stop(process):
        if process.poll() is None:
            os.killpg(process.pid, signal.SIGTERM)
            try:
                process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                os.killpg(process.pid, signal.SIGKILL)
                process.wait(timeout=5)

    def close(self):
        for child in reversed(self.children):
            self.stop(child)
        for log in self.logs:
            log.close()
        (self.artifacts / "cleanup.json").write_text(json.dumps({"ownedProcesses": [{"pid": p.pid, "returncode": p.poll()} for p in self.children], "complete": all(p.poll() is not None for p in self.children)}, indent=2) + "\n", encoding="utf-8")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=Path(__file__).resolve().parents[3])
    parser.add_argument("--artifacts", type=Path, required=True)
    parser.add_argument("--script", type=Path, help="JSON array of native input actions; otherwise read JSON lines from stdin")
    parser.add_argument("--output-fixtures", type=Path, help="Copy fixture.* files into the disposable project and emit real file.changed events")
    args = parser.parse_args()
    root, artifacts = args.root.resolve(), args.artifacts.resolve()
    if artifacts == root or root in artifacts.parents:
        parser.error("Artifacts must be outside the project repository")
    desktop = Desktop(root, artifacts, args.output_fixtures)
    failures = 0
    try:
        print(json.dumps({"ready": desktop.start()}), flush=True)
        actions = json.loads(args.script.read_text()) if args.script else (json.loads(line) for line in sys.stdin if line.strip())
        for request in actions:
            try:
                response = {"request": request, "result": desktop.action(request)}
            except Exception as error:
                response = {"request": request, "error": str(error)}
                failures += 1
            with (artifacts / "actions.jsonl").open("a", encoding="utf-8") as output:
                output.write(json.dumps(response) + "\n")
            print(json.dumps(response), flush=True)
    finally:
        desktop.close()
    if args.script and failures:
        raise SystemExit(1)


if __name__ == "__main__":
    main()
