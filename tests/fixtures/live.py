#!/usr/bin/env python3
"""Exercise installed native harnesses through swarmie's stdio daemon."""
import uuid
import argparse
import json
import os
from pathlib import Path
import queue
import shutil
import subprocess
import threading
import tempfile
import time

ROOT = Path(__file__).resolve().parents[2]


class Client:
    def __init__(self, binary, directory, env, timeout):
        self.timeout = timeout
        self.serial = 0
        self.queue = queue.Queue()
        self.events = []
        self.stderr = (directory / "daemon.stderr").open("a")
        self.trace = (directory / "events.jsonl").open("a")
        self.process = subprocess.Popen(
            [str(binary), "--state-dir", str(directory / "sessions"), "serve"],
            stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=self.stderr,
            text=True, env=env,
        )
        def read():
            try:
                for line in self.process.stdout:
                    self.queue.put(json.loads(line))
            finally:
                self.queue.put(None)
        threading.Thread(target=read, daemon=True).start()

    def next(self, deadline):
        try:
            value = self.queue.get(timeout=max(0.001, deadline - time.monotonic()))
        except queue.Empty:
            raise TimeoutError("native harness deadline") from None
        if value is None:
            raise RuntimeError("daemon exited")
        if value.get("method") == "session/event":
            event = value["params"]["event"]
            self.events.append(event)
            # Preserve semantic evidence without dumping native request internals.
            self.trace.write(json.dumps({k: v for k, v in event.items() if k != "raw"}) + "\n")
            self.trace.flush()
        return value

    def call(self, method, params=None):
        self.serial += 1
        self.process.stdin.write(json.dumps({"jsonrpc": "2.0", "id": self.serial, "method": method, "params": params or {}}) + "\n")
        self.process.stdin.flush()
        deadline = time.monotonic() + self.timeout
        while True:
            value = self.next(deadline)
            if value.get("id") == self.serial:
                if "error" in value:
                    raise RuntimeError(json.dumps(value["error"]))
                return value.get("result")

    def turn(self, session, prompt, expected, require_tool=False):
        first = len(self.events)
        self.call("session/prompt", {"sessionId": session, "text": prompt, "idempotencyKey": str(uuid.uuid4())})
        deadline = time.monotonic() + self.timeout
        while not any(e["type"] == "turn.ended" for e in self.events[first:]):
            self.next(deadline)
        events = self.events[first:]
        terminal = next(e for e in events if e["type"] == "turn.ended")
        assert terminal["data"]["outcome"] == "completed", terminal
        text = "".join(e["data"]["text"] for e in events if e["type"] == "text.delta")
        assert expected in text, "expected model output missing"
        if require_tool:
            assert any(e["type"].startswith("tool.") for e in events), "native tool was not exercised"
        sequences = [e["sequence"] for e in events]
        assert sequences == sorted(set(sequences)), "event sequence is not strictly ordered"
        return len(events)

    def close(self):
        if self.process.poll() is None:
            self.process.stdin.close()
            try:
                self.process.wait(timeout=10)
            except subprocess.TimeoutExpired:
                self.process.kill()
                self.process.wait()
        self.stderr.close()
        self.trace.close()


def executable(name):
    local = ROOT / "tests/fixtures/harnesses/node_modules/.bin" / name
    found = str(local) if local.exists() else shutil.which(name)
    if not found:
        raise RuntimeError(f"Install {name} before running the live fixture")
    return found


def run(harness, args):
    directory = args.artifacts / harness
    directory.mkdir(parents=True, exist_ok=True)
    env = os.environ.copy()
    env.pop("CLAUDECODE", None)
    env.pop("ANTHROPIC_AUTH_TOKEN", None)
    model = env.get("SWARMIE_FIXTURE_MODEL", "fixture-model")
    base = env.get("SWARMIE_FIXTURE_BASE_URL", "http://127.0.0.1:8091").rstrip("/")
    options = {"harness": harness, "cwd": str(directory), "config": {"executable": executable(harness)}}
    proxy = None
    client = None
    proxy_log = None
    try:
        if harness == "pi":
            agent_dir = directory / "pi-agent"
            agent_dir.mkdir(exist_ok=True)
            (agent_dir / "models.json").write_text(json.dumps({"providers": {"fixture": {
                "baseUrl": base + "/v1", "api": "openai-responses", "apiKey": "${SWARMIE_FIXTURE_API_KEY}",
                "models": [{"id": model, "name": "Fixture model", "reasoning": False, "input": ["text"], "contextWindow": 32768, "maxTokens": 1024}]
            }}}))
            env["PI_CODING_AGENT_DIR"] = str(agent_dir)
            options["model"] = "fixture/" + model
            options["config"]["args"] = ["--mode", "rpc", "--provider", "fixture", "--no-extensions", "--no-skills", "--no-prompt-templates", "--tools", "read", "--system-prompt", "Follow the user's instruction exactly. Use read when requested."]
        elif harness == "claude":
            proxy_log = (directory / "proxy.stdout").open("w")
            proxy = subprocess.Popen(["python3", str(ROOT / "tests/fixtures/inference_proxy.py"), "--port", "0", "--upstream", base, "--model", model], stdout=subprocess.PIPE, stderr=proxy_log, text=True, env=env)
            ready = queue.Queue()
            threading.Thread(target=lambda: ready.put(proxy.stdout.readline()), daemon=True).start()
            address = ready.get(timeout=10).strip().split(" ")[-1]
            assert address.startswith("http://127.0.0.1:"), "proxy did not start"
            env.update({"ANTHROPIC_BASE_URL": address, "ANTHROPIC_API_KEY": "fixture-local", "ANTHROPIC_MODEL": model})
            options["model"] = model
            options["config"]["args"] = ["--print", "--verbose", "--input-format", "stream-json", "--output-format", "stream-json", "--include-partial-messages", "--permission-prompt-tool", "stdio", "--bare", "--tools", "", "--system-prompt", "Follow the user's instruction exactly."]
        elif harness == "opencode":
            config = directory / "opencode.json"
            config.write_text(json.dumps({"provider": {"fixture": {"npm": "@ai-sdk/openai-compatible", "name": "Fixture", "options": {"baseURL": base + "/v1", "apiKey": "{env:SWARMIE_FIXTURE_API_KEY}"}, "models": {model: {"name": "Fixture model", "limit": {"context": 32768, "output": 1024}}}}}, "agent": {"build": {"prompt": "Follow the user instruction exactly.", "tools": {"*": False}, "permission": {"*": "deny"}}}, "permission": {"*": "deny"}}))
            env["OPENCODE_CONFIG"] = str(config)
            options["model"] = "fixture/" + model
        client = Client(args.binary, directory, env, args.timeout)
        client.call("runtime/initialize")
        snapshot = client.call("session/create", options)
        session = snapshot["id"]
        count = client.turn(session, "Reply exactly: SWARMIE_LIVE_OK", "SWARMIE_LIVE_OK")
        if harness == "pi":
            (directory / "smoke.txt").write_text("SWARMIE_TOOL_OK\n")
            count += client.turn(session, "Use read to read smoke.txt and reply exactly with its content.", "SWARMIE_TOOL_OK", require_tool=True)
        else:
            count += client.turn(session, "Reply exactly: SWARMIE_SECOND_OK", "SWARMIE_SECOND_OK")
        client.call("session/close", {"sessionId": session})
        client.close()
        client = Client(args.binary, directory, env, args.timeout)
        resumed = client.call("session/resume", {"sessionId": session})
        assert resumed["id"] == session and resumed["native"] == snapshot["native"], "native session reference changed"
        count += client.turn(session, "Reply exactly: SWARMIE_RESUME_OK", "SWARMIE_RESUME_OK")
        client.call("session/close", {"sessionId": session})
        print(f"PASS {harness}: two turns, daemon restart, native resume; {count} ordered events", flush=True)
    finally:
        if client:
            client.close()
        if proxy:
            proxy.terminate()
            try:
                proxy.wait(timeout=5)
            except subprocess.TimeoutExpired:
                proxy.kill()
                proxy.wait()
        if proxy_log:
            proxy_log.close()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--harness", action="append", choices=["pi", "claude", "opencode", "codex"])
    parser.add_argument("--binary", type=Path, default=ROOT / "target/debug/swarmie")
    parser.add_argument("--timeout", type=int, default=180)
    artifact_root = Path(tempfile.gettempdir()) / 'swarmie-live-tests' / ('live-' + time.strftime('%Y%m%d-%H%M%S'))
    parser.add_argument("--artifacts", type=Path, default=artifact_root)
    args = parser.parse_args()
    if not args.binary.exists():
        parser.error("Build swarmie first: cargo build --workspace")
    if not os.environ.get("SWARMIE_FIXTURE_API_KEY") and args.harness != ["codex"]:
        parser.error("SWARMIE_FIXTURE_API_KEY is required for endpoint tests")
    for harness in args.harness or ["pi", "claude", "opencode"]:
        run(harness, args)
    print(f"Evidence: {args.artifacts}")


if __name__ == "__main__":
    main()
