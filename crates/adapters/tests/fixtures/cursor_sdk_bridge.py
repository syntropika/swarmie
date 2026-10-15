#!/usr/bin/env python3
"""Fixture for the public Cursor sdk.v1 Connect JSON protocol, without paid calls."""
import http.server
import json
import os
import struct
import sys
import threading
from pathlib import Path

workspace = Path(sys.argv[sys.argv.index("--workspace") + 1])
token = "fixture-local-bridge-token"
key = "fixture-cursor-account-key"
token_file = workspace / "bridge-token"
token_file.write_text(token)
if os.environ.get("FIXTURE_BAD_TOKEN"):
    token_file.write_text("x" * 5000)
token_file.chmod(0o600)
cancelled = threading.Event()
log = workspace / "requests.jsonl"
(workspace / "bridge-pid").write_text(str(os.getpid()))
lock = threading.Lock()


def frame(value, flags=0):
    payload = json.dumps(value).encode()
    return struct.pack(">BI", flags, len(payload)) + payload


class Handler(http.server.BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def log_message(self, *args):
        pass

    def do_POST(self):
        assert self.headers["Authorization"] == "Bearer " + token
        assert self.headers["Connect-Protocol-Version"] == "1"
        data = self.rfile.read(int(self.headers["Content-Length"]))
        method = self.path.rsplit("/", 1)[1]
        if method == "Send":
            assert self.headers["Content-Type"] == "application/connect+json"
            flags, length = struct.unpack(">BI", data[:5])
            assert flags == 0 and length == len(data) - 5
            request = json.loads(data[5:])
        else:
            assert self.headers["Content-Type"] == "application/json"
            request = json.loads(data)
        with lock:
            with log.open("a") as target:
                target.write(json.dumps({"method": method, "request": request}) + "\n")
        if method in ("Me", "ListModels"):
            assert request["options"]["apiKey"] == key
        if method in ("CreateAgent", "ResumeAgent"):
            assert request["options"]["apiKey"] == key
            assert request["options"]["local"]["cwd"] == [str(workspace.resolve())]
            assert request["options"]["local"]["sandboxOptions"] == {"enabled": True}
            assert request["options"]["local"]["autoReview"] is True
            assert "cloud" not in request["options"]
            return self.reply({"agentId": request.get("agentId", "native-agent"), "model": request["options"]["model"]})
        if method == "Ping":
            return self.reply({"message": "pong"})
        if method == "GetVersion":
            return self.reply({"bridgeVersion": "1.0.0", "protocolVersion": "sdk.v2" if os.environ.get("FIXTURE_BAD_PROTOCOL") else "sdk.v1", "capabilities": []})
        if method == "ListModels":
            return self.reply({"items": [{"id": "composer-2", "displayName": "Composer 2", "description": "Fixture model"}, {"id": "model-b", "displayName": "Model B"}]})
        if method == "Me":
            return self.reply({"user": {"userEmail": "fixture@example.invalid"}})
        if method == "CancelRun":
            assert request["runId"] == "native-run"
            cancelled.set()
            return self.reply({})
        if method == "WaitLiveRun":
            return self.reply({"result": self.result()})
        if method == "CloseAgent":
            return self.reply({})
        if method == "Shutdown":
            self.reply({})
            threading.Thread(target=server.shutdown, daemon=True).start()
            return
        if method in ("ListAgents", "GetAgent", "ListAgentMessages") and "--history-fixture" in sys.argv:
            source = Path(sys.argv[sys.argv.index("--history-fixture") + 1])
            history = json.loads(source.read_text())
            options = request.get("options", {})
            assert "apiKey" not in options
            if method == "ListAgents":
                assert options["runtime"] == "RUNTIME_LOCAL"
                cursor = options.get("cursor", "")
                page = history["pages"].get(cursor, {"items": []})
                return self.reply(page)
            if method == "GetAgent":
                agent = history.get("agents", {}).get(request["agentId"])
                if agent is None:
                    return self.reply({"code": "not_found", "message": "SDK agent not found"}, 404)
                return self.reply({"agent": agent})
            assert options["runtime"] == "RUNTIME_LOCAL"
            messages = history.get("messages", {}).get(request["agentId"], [])
            offset = 0 if history.get("repeatMessages") else options["offset"]
            return self.reply({"messages": messages[offset:offset + options["limit"]]})
        if method == "Send":
            return self.send(request)
        raise AssertionError("Unexpected fixture RPC: " + method)

    def reply(self, value, status=200):
        body = json.dumps(value).encode()
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def result(self, status="RUN_LIFECYCLE_STATUS_FINISHED"):
        return {"agentId": "native-agent", "runId": "native-run", "status": status, "result": "Hello", "usage": {"inputTokens": "12", "outputTokens": "3", "totalTokens": "15"}}

    def send(self, request):
        assert request["agentId"] == "native-agent"
        assert request["options"]["enableDeltas"] is True
        if request["message"]["text"] == "ambiguous":
            # A native side effect may have happened before an HTTP failure.
            return self.reply({"code": "internal", "message": "unknown accepted state"}, 503)
        self.send_response(200)
        self.send_header("Content-Type", "application/connect+json")
        self.send_header("Transfer-Encoding", "chunked")
        self.end_headers()
        self.wfile.flush()

        def chunk(data):
            # Split every Connect envelope across multiple HTTP chunks.
            for part in (data[:2], data[2:6], data[6:]):
                self.wfile.write(f"{len(part):x}\r\n".encode() + part + b"\r\n")
                self.wfile.flush()

        text = request["message"]["text"]
        if text == "unresolved":
            chunk(frame({}, 2))
            self.wfile.write(b"0\r\n\r\n")
            self.wfile.flush()
            return
        chunk(frame({}))  # idle keepalive
        chunk(frame({"futureEnvelope": {"ignored": True}}))
        chunk(frame({"sdkMessage": {"type": "system", "message": {"subtype": "init", "run_id": "native-run", "agent_id": "native-agent"}}}))
        if text == "disconnect":
            # Closing a Send stream leaves the run alive; WaitLiveRun resolves it.
            self.close_connection = True
            return
        if text == "failed":
            status = "RUN_LIFECYCLE_STATUS_ERROR"
            chunk(frame({"sdkMessage": {"type": "status", "message": {"status": "ERROR", "message": "Fixture failure: " + key + " " + token}}}))
        elif text == "cancel":
            assert cancelled.wait(5), "Adapter failed to cancel the native run"
            status = "RUN_LIFECYCLE_STATUS_CANCELLED"
        else:
            status = "RUN_LIFECYCLE_STATUS_FINISHED"
            for value in [
                {"sdkMessage": {"type": "assistant", "message": {"message": {"content": [{"type": "text", "text": "Hello"}]}}}},
                {"interactionUpdate": {"type": "thinking-delta", "update": {"text": "Inspect"}}},
                {"interactionUpdate": {"type": "text-delta", "update": {"text": "Hello"}}},
                {"interactionUpdate": {"type": "tool-call-started", "update": {"callId": "tool-1", "toolCall": {"type": "read", "args": {"path": "README.md"}}}}},
                {"interactionUpdate": {"type": "tool-call-completed", "update": {"callId": "tool-1", "toolCall": {"type": "read", "args": {"path": "README.md"}, "result": {"status": "success", "value": "fixture"}}}}},
                {"sdkMessage": {"type": "tool_call", "message": {"call_id": "tool-1", "name": "read", "status": "completed", "args": {"path": "README.md"}, "result": {"status": "success", "value": "fixture"}}}},
                {"sdkMessage": {"type": "assistant", "message": {"message": {"content": [{"type": "text", "text": "Hello"}]}}}},
            ]:
                chunk(frame(value))
        chunk(frame({"result": {"agentId": "native-agent", "runId": "native-run", "status": status, "result": self.result(status)}}))
        chunk(frame({"done": {"agentId": "native-agent", "runId": "native-run"}}))
        chunk(frame({}, 2))
        self.wfile.write(b"0\r\n\r\n")
        self.wfile.flush()


server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Handler)
server.daemon_threads = True
port = server.server_address[1]
ready = {"schemaVersion": 1, "transport": "tcp", "protocol": "connect", "url": f"http://127.0.0.1:{port}", "authTokenFile": str(token_file)}
if os.environ.get("FIXTURE_BAD_DISCOVERY"):
    ready["schemaVersion"] = 2
if os.environ.get("FIXTURE_REMOTE_DISCOVERY"):
    ready["url"] = "http://192.0.2.1:80"
print("diagnostic containing " + key, file=sys.stderr, flush=True)
if os.environ.get("FIXTURE_EXIT_EARLY"):
    sys.exit(2)
if os.environ.get("FIXTURE_LONG_LINE"):
    print("x" * 65537, file=sys.stderr, flush=True)
if os.environ.get("FIXTURE_NEVER_READY"):
    threading.Event().wait(60)
print("cursor-sdk-bridge ready " + json.dumps(ready), file=sys.stderr, flush=True)
server.serve_forever()
server.server_close()
