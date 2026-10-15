"""Exact native OpenCode OAuth HTTP peer with isolated fake credential writes."""
import base64
import json
import os
import pathlib
import sys
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.parse import parse_qs, urlparse

mode = sys.argv[1]
root = pathlib.Path.cwd()
lock = threading.Lock()
expected = "Basic " + base64.b64encode((os.environ["OPENCODE_SERVER_USERNAME"] + ":" + os.environ["OPENCODE_SERVER_PASSWORD"]).encode()).decode()


class Handler(BaseHTTPRequestHandler):
    def log_message(self, *args):
        pass

    def response(self, status, value):
        payload = json.dumps(value).encode()
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(payload)))
        self.end_headers()
        self.wfile.write(payload)

    def request(self):
        url = urlparse(self.path)
        body = None
        if self.command == "POST":
            body = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
        record = {"method": self.command, "path": url.path, "body": body, "authenticated": self.headers.get("Authorization") == expected, "privateDirectory": parse_qs(url.query).get("directory") == [str(root)]}
        with lock:
            with (root / "auth-requests.jsonl").open("a") as output:
                output.write(json.dumps(record) + "\n")
        if not record["authenticated"] or not record["privateDirectory"]:
            self.response(401, {"message": "Access denied"})
            return
        if url.path == "/provider/auth":
            if mode == "oversized":
                self.response(200, {"secret": "fixture-private-secret" * 10000})
                return
            prompt = [{"type": "text", "key": "unexpected", "message": "Unknown prompt"}] if mode == "unsupported" else []
            self.response(200, {
                "openai": [{"type": "api", "label": "API key"}, {"type": "oauth", "label": "Browser"}, {"type": "oauth", "label": "ChatGPT headless device", "prompts": prompt}],
                "github-copilot": [{"type": "api", "label": "API key"}, {"type": "oauth", "label": "GitHub Copilot device", "prompts": [{"type": "select", "key": "deploymentType", "options": [{"value": "github.com"}, {"value": "enterprise"}]}, {"type": "text", "key": "enterpriseUrl", "when": {"key": "deploymentType", "op": "eq", "value": "enterprise"}}]}],
            })
        elif url.path.endswith("/oauth/authorize"):
            if mode == "error":
                self.response(400, {"message": "Authorization rejected fixture-private-secret"})
                return
            provider = url.path.split("/")[2]
            url = "https://github.com/login/device" if provider == "github-copilot" else "https://auth.openai.com/codex/device"
            if mode == "bad_url":
                url = "https://attacker.example/credential"
            self.response(200, {"url": url, "method": "code" if mode in ("code", "slow_code") else "auto", "instructions": "Do not expose fixture-private-secret" if mode in ("code", "slow_code") else "Enter code: ABCD-1234"})
        elif url.path.endswith("/oauth/callback"):
            if mode in ("slow", "slow_code"):
                time.sleep(10)
            if mode in ("code", "slow_code") and body.get("code") != "private-test-code":
                self.response(400, {"message": "Rejected code " + body.get("code", "fixture-private-secret")})
                return
            if mode != "code":
                time.sleep(0.2)
            provider = url.path.split("/")[2]
            directory = pathlib.Path(os.environ["XDG_DATA_HOME"]) / "opencode"
            directory.mkdir(parents=True, exist_ok=True)
            path = directory / "auth.json"
            path.write_text(json.dumps({provider: {"type": "oauth", "access": "fixture-private-secret", "refresh": "fixture-refresh", "expires": 9999999999999}}))
            path.chmod(0o600)
            self.response(200, True)
        else:
            self.response(400, {"message": "Unexpected non-authentication endpoint"})

    do_GET = request
    do_POST = request


server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
server.daemon_threads = True
print("opencode server listening on http://127.0.0.1:" + str(server.server_address[1]), flush=True)
server.serve_forever()
