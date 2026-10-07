#!/usr/bin/env python3
"""HTTP/SSE peer covering stale idle, user roles and streamed assistant output."""
import argparse
import json
import queue
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

session = "fixture-session"
events = queue.Queue()
status = {}
first_health = True


def emit(kind, **properties):
    events.put({"type": kind, "properties": dict(sessionID=session, **properties)})


class Handler(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def log_message(self, *_):
        pass

    def reply(self, value):
        data = json.dumps(value).encode()
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        try:
            self.wfile.write(data)
        except (BrokenPipeError, ConnectionResetError):
            pass

    def do_GET(self):
        path = self.path.split("?", 1)[0]
        if path == "/global/health":
            global first_health
            if first_health:
                first_health = False
                time.sleep(2)
            self.reply({"healthy": True})
        elif path == "/session/status":
            self.reply(status)
        elif path == "/event":
            self.send_response(200)
            self.send_header("Content-Type", "text/event-stream")
            self.end_headers()
            self.wfile.write(b": connected\n\n")
            self.wfile.flush()
            try:
                while True:
                    try:
                        value = events.get(timeout=0.1)
                        data = "data: " + json.dumps(value) + "\n\n"
                    except queue.Empty:
                        data = ": heartbeat\n\n"
                    self.wfile.write(data.encode())
                    self.wfile.flush()
            except (BrokenPipeError, ConnectionResetError):
                pass
        else:
            self.reply({"id": session})

    def do_POST(self):
        body = json.loads(self.rfile.read(int(self.headers.get("Content-Length", "0"))) or "{}")
        path = self.path.split("?", 1)[0]
        if path == "/session":
            self.reply({"id": session})
        elif path.endswith("/prompt_async"):
            text = body["parts"][0]["text"]
            status[session] = {"type": "busy"}
            emit("message.updated", info={"sessionID": session, "id": "user", "role": "user"})
            emit("message.part.updated", part={"sessionID": session, "id": "user-part", "messageID": "user", "type": "text", "text": text})
            emit("session.status", status={"type": "busy"})
            if text != "hold":
                threading.Thread(target=self.complete, daemon=True).start()
            self.reply(None)
        elif path.endswith("/abort"):
            status.clear()
            emit("session.status", status={"type": "idle"})
            threading.Thread(target=self.duplicate_idle, daemon=True).start()
            self.reply(True)
        else:
            self.reply(True)

    def complete(self):
        time.sleep(0.4)
        emit("message.updated", info={"sessionID": session, "id": "assistant", "role": "assistant"})
        emit("message.part.delta", messageID="assistant", partID="text-part", field="text", delta="Fixture output")
        emit("message.part.updated", part={"sessionID": session, "messageID": "assistant", "id": "text-part", "type": "text", "text": "Fixture output"})
        status.clear()
        emit("session.status", status={"type": "idle"})

    def duplicate_idle(self):
        time.sleep(0.15)
        emit("session.idle")


parser = argparse.ArgumentParser()
parser.add_argument("--port", type=int, required=True)
args = parser.parse_args()
ThreadingHTTPServer(("127.0.0.1", args.port), Handler).serve_forever()
