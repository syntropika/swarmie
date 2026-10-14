from __future__ import annotations
import json
import math
import threading
from typing import Any, Iterator
from .native import Handle, execute, invoke, RpcError, ProtocolError
from .generated import InputSpawnOptions, InputPromptContent, SessionSnapshot, RuntimeEvent, SubmissionReceipt

def milliseconds(seconds):
    if not math.isfinite(seconds) or seconds <= 0: raise ValueError("Timeout must be positive")
    return max(1, math.ceil(seconds * 1000))

def decode_response(value: Any, request_id: str) -> Any:
    return json.loads(execute({"action": "decode", "response_json": json.dumps(value), "request_id": request_id}))

class HttpTransport(Handle):
    def __init__(self, url: str, token: str | None = None, timeout: float = 30, max_response_bytes: int = 16 * 1024 * 1024):
        super().__init__({"action": "create_http", "config": {"url": url, "token": token, "timeout_ms": milliseconds(timeout), "max_response_bytes": max_response_bytes}})
    def request(self, method, params, timeout=None, cancel=None):
        return invoke({"action": "invoke", "client": self.id, "request": {"method": method, "params_json": json.dumps(params), "timeout_ms": milliseconds(timeout) if timeout is not None else None}}, cancel)

class StdioTransport(HttpTransport):
    def __init__(self, command: list[str], timeout: float = 30):
        if not command: raise ValueError("Command must not be empty")
        Handle.__init__(self, {"action": "create_stdio", "config": {"command": command[0], "args": command[1:], "timeout_ms": milliseconds(timeout), "max_response_bytes": 16 * 1024 * 1024}})
    def close(self): self.dispose()

class Client:
    def __init__(self, transport: HttpTransport): self.transport = transport
    @classmethod
    def http(cls, url: str, **options): return cls(HttpTransport(url, **options))
    def dispose(self): self.transport.dispose()
    def __enter__(self): return self
    def __exit__(self, *_): self.dispose()
    def call(self, method: str, params: dict[str, Any] | None = None, timeout: float | None = None, cancel: threading.Event | None = None) -> Any:
        return self.transport.request(method, params if params is not None else {}, timeout, cancel)
    def initialize(self) -> dict[str,Any]: return self.call("runtime/initialize")
    def schema(self) -> dict[str,Any]: return self.call("runtime/schema")
    def discover(self) -> list[dict[str,Any]]: return self.call("runtime/discover")
    def spawn(self,options: InputSpawnOptions) -> SessionSnapshot: return self.call("session/create",options)
    def prompt(self,session_id: str,prompt: str | list[InputPromptContent],idempotency_key: str) -> SubmissionReceipt: return self.call("session/prompt",{"sessionId":session_id,"idempotencyKey":idempotency_key,**({"text":prompt} if isinstance(prompt,str) else {"content":prompt})})
    def submission(self,session_id: str,idempotency_key: str) -> SubmissionReceipt | None: return self.call("session/submission",{"sessionId":session_id,"idempotencyKey":idempotency_key})
    def reconcile_submission(self,session_id: str,idempotency_key: str) -> SubmissionReceipt | None: return self.call("session/submission/reconcile",{"sessionId":session_id,"idempotencyKey":idempotency_key})
    def close(self,session_id: str): return self.call("session/close",{"sessionId":session_id})
    def events(self, session_id: str, after_sequence: int = 0, timeout: float = 600, poll_interval: float = .2, cancel: threading.Event | None = None) -> Iterator[RuntimeEvent]:
        stream = Handle({"action": "open_events", "client": self.transport.id, "session_id": session_id, "options": {"after_sequence": after_sequence, "timeout_ms": milliseconds(timeout), "poll_interval_ms": milliseconds(poll_interval)}})
        try:
            while True: yield invoke({"action": "next_event", "stream": stream.id}, cancel)
        finally: stream.dispose()
    def wait_for_turn(self, session_id: str, **options) -> list[RuntimeEvent]:
        return invoke({"action": "wait_turn", "client": self.transport.id, "session_id": session_id, "options": {"after_sequence": options.get("after_sequence", 0), "timeout_ms": milliseconds(options.get("timeout", 600)), "poll_interval_ms": milliseconds(options.get("poll_interval", .2))}}, options.get("cancel"))
