#!/usr/bin/env python3
"""Independent bounded HTTP/SSE peer for diagnostics integration tests."""
import base64
import http.server
import json
import os
import queue
import sys
import threading
import time
import urllib.parse

streams = []
lock = threading.Lock()
status = "idle"

def echo(handler):
    values = [os.getenv("DIAGNOSTIC_ENV", ""), handler.headers.get("Authorization", "").removeprefix("Basic ")]
    values.extend(value for _, value in urllib.parse.parse_qsl(urllib.parse.urlsplit(handler.path).query) if value != os.getcwd())
    config = json.loads(os.getenv("OPENCODE_CONFIG_CONTENT", "{}"))
    for server in config.get("mcp", {}).values():
        values.extend(server.get("headers", {}).values())
        values.extend(server.get("environment", {}).values())
    return {"echo": " / ".join(values), "password": os.getenv("OPENCODE_SERVER_PASSWORD", ""),
            "headers": dict(handler.headers), "url": "http://user:embedded-password@example.test/path?private=secret-query"}

class Handler(http.server.BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"
    def log_message(self, *_):
        pass
    def reply(self, value, code=200):
        data = json.dumps(value).encode()
        self.send_response(code)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)
    def do_GET(self):
        path = urllib.parse.urlsplit(self.path).path
        if path == "/event":
            self.send_response(200)
            self.send_header("Content-Type", "text/event-stream")
            self.end_headers()
            self.wfile.flush()
            events = queue.Queue()
            with lock:
                streams.append(events)
            try:
                while True:
                    value = events.get(timeout=30)
                    self.wfile.write(b"data: " + json.dumps(value).encode() + b"\n\n")
                    self.wfile.flush()
            except (BrokenPipeError, ConnectionResetError, queue.Empty):
                pass
            finally:
                with lock:
                    streams.remove(events)
        elif path == "/global/health":
            self.reply({"healthy": True, **echo(self)})
        elif path == "/config/providers":
            self.reply({"providers": [], "default": {}})
        elif path == "/session/status":
            self.reply({"diagnostic-session": {"type": status}})
        elif path == "/command":
            self.reply({"large": "x" * (17 * 1024 * 1024)})
        elif path.endswith("/message"):
            self.reply([])
        else:
            self.reply({"id": "diagnostic-session", **echo(self)})
    def do_POST(self):
        global status
        path = urllib.parse.urlsplit(self.path).path
        body = json.loads(self.rfile.read(int(self.headers.get("Content-Length", "0"))) or b"{}")
        if path.endswith("/prompt_async"):
            status = "busy"
            native_echo = echo(self)
            oversized = any(part.get("text") == "oversized" for part in body.get("parts", []))
            self.reply(None)
            def finish():
                global status
                time.sleep(0.18)
                events = [{"type": "message.part.delta", "properties": {"sessionID": "diagnostic-session", "partID": "part", "delta": "ready", **native_echo}},
                          {"type": "session.idle", "properties": {"sessionID": "diagnostic-session"}}]
                if oversized:
                    events = [{"type": "large", "properties": {"sessionID": "diagnostic-session", "large": "x" * (9 * 1024 * 1024)}}]
                status = "idle"
                with lock:
                    for stream in streams:
                        for event in events:
                            stream.put(event)
            threading.Thread(target=finish, daemon=True).start()
        elif path.endswith("/abort"):
            status = "idle"
            self.reply(None)
        else:
            self.reply({"id": "diagnostic-session", "received": body, **echo(self)})

port = int(sys.argv[sys.argv.index("--port") + 1]) if "--port" in sys.argv else 0
password = os.getenv("OPENCODE_SERVER_PASSWORD")
if password:
    with open("test-auth.json", "w") as receipt:
        json.dump({"password": password, "encoded": base64.b64encode(("opencode:" + password).encode()).decode()}, receipt)
server = http.server.ThreadingHTTPServer(("127.0.0.1", port), Handler)
server.daemon_threads = True
if "--ready" in sys.argv:
    with open(sys.argv[sys.argv.index("--ready") + 1], "w") as ready:
        ready.write(str(server.server_port))
server.serve_forever()
