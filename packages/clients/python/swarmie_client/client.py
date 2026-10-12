from __future__ import annotations
import http.client, json, math, os, queue, select, subprocess, threading, time, uuid
from typing import Any, Iterator, Protocol
from .generated import InputSpawnOptions, InputPromptContent, SessionSnapshot, RuntimeEvent, SubmissionReceipt

class RpcError(Exception):
    def __init__(self, code: int, message: str, data: Any = None):
        super().__init__(message)
        self.code, self.data = code, data

class ProtocolError(Exception): pass

def decode_response(value: Any, request_id: str) -> Any:
    if not isinstance(value, dict) or value.get("jsonrpc") != "2.0" or value.get("id") != request_id:
        raise ProtocolError("JSON-RPC response identity mismatch")
    if "error" in value:
        error = value["error"]
        if not isinstance(error, dict) or not isinstance(error.get("code"), int) or not isinstance(error.get("message"), str):
            raise ProtocolError("Malformed JSON-RPC error")
        raise RpcError(error["code"], error["message"], error.get("data"))
    if "result" not in value: raise ProtocolError("JSON-RPC response has no result")
    return value["result"]

class Transport(Protocol):
    def request(self, method: str, params: dict[str, Any], timeout: float | None = None) -> Any: ...

class HttpTransport:
    def __init__(self, url: str, token: str | None = None, timeout: float = 30, max_response_bytes: int = 16 * 1024 * 1024):
        if not url.startswith(("http://", "https://")): raise ValueError("Use an HTTP or HTTPS endpoint")
        if not math.isfinite(timeout) or timeout <= 0 or max_response_bytes <= 0: raise ValueError("Timeout and byte limit must be positive")
        from urllib.parse import urlsplit, urlunsplit
        parts=urlsplit(url)
        self.url=urlunsplit((parts.scheme, parts.netloc, "/rpc" if parts.path in ("", "/") else parts.path, parts.query, ""))
        self._token, self.timeout, self.max_response_bytes = token, timeout, max_response_bytes

    def request(self, method: str, params: dict[str, Any], timeout: float | None = None) -> Any:
        request_id = str(uuid.uuid4())
        body=json.dumps({"jsonrpc":"2.0", "id":request_id, "method":method, "params":params}).encode()
        headers={"Content-Type":"application/json"}
        if self._token: headers["Authorization"]="Bearer " + self._token
        from urllib.parse import urlsplit
        parts=urlsplit(self.url)
        duration=min(timeout if timeout is not None else self.timeout,self.timeout)
        if not math.isfinite(duration) or duration <= 0: raise ValueError("Timeout must be positive")
        deadline=time.monotonic()+duration
        connection_type=http.client.HTTPSConnection if parts.scheme == "https" else http.client.HTTPConnection
        connection=connection_type(parts.hostname,parts.port,timeout=duration)
        response=None
        try:
            connection.request("POST",parts.path+("?"+parts.query if parts.query else ""),body=body,headers=headers)
            sock=connection.sock
            response=connection.getresponse()
            if not 200 <= response.status < 300: raise ProtocolError(f"Runtime HTTP status {response.status}")
            raw=bytearray()
            while True:
                remaining=deadline-time.monotonic()
                if remaining <= 0: raise TimeoutError("Runtime HTTP request timed out")
                if sock: sock.settimeout(remaining)
                chunk=response.read1(min(65536,self.max_response_bytes+1-len(raw)))
                if not chunk: break
                raw.extend(chunk)
                if len(raw) > self.max_response_bytes: raise ProtocolError("Runtime response exceeded the configured byte limit")
        finally:
            if response: response.close()
            connection.close()
        try: value=json.loads(raw)
        except (json.JSONDecodeError, UnicodeDecodeError): raise ProtocolError("Runtime response is not JSON") from None
        return decode_response(value, request_id)

class StdioTransport:
    """Own a daemon process; calls are serialized and bounded by a deadline."""
    def __init__(self, command: list[str], timeout: float = 30):
        if not math.isfinite(timeout) or timeout <= 0: raise ValueError("Timeout must be positive")
        self.timeout=timeout
        self._process=subprocess.Popen(command, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL)
        os.set_blocking(self._process.stdin.fileno(),False)
        self._responses: queue.Queue[Any]=queue.Queue()
        self._lock=threading.Lock()
        threading.Thread(target=self._reader,daemon=True).start()

    def _reader(self):
        try:
            while self._process.stdout:
                raw=self._process.stdout.readline(16 * 1024 * 1024 + 1)
                if not raw: break
                if len(raw) > 16 * 1024 * 1024: raise ProtocolError("Runtime stdio frame exceeded 16 MiB")
                value=json.loads(raw)
                if "id" in value: self._responses.put(value)
        except Exception as error: self._responses.put(error)
        finally: self._responses.put(ProtocolError("Runtime process exited"))

    def request(self, method: str, params: dict[str, Any], timeout: float | None = None) -> Any:
        duration=min(timeout if timeout is not None else self.timeout,self.timeout)
        if not math.isfinite(duration) or duration <= 0: raise ValueError("Timeout must be positive")
        deadline=time.monotonic()+duration
        if not self._lock.acquire(timeout=duration): raise TimeoutError("Runtime stdio request timed out waiting for another call")
        try:
            if self._process.poll() is not None: raise ProtocolError("Runtime process is closed")
            request_id=str(uuid.uuid4())
            frame=(json.dumps({"jsonrpc":"2.0","id":request_id,"method":method,"params":params})+"\n").encode()
            offset=0
            try:
                while offset < len(frame):
                    remaining=deadline-time.monotonic()
                    if remaining <= 0: raise TimeoutError("Runtime stdio request timed out")
                    ready=select.select([], [self._process.stdin], [], remaining)[1]
                    if not ready: raise TimeoutError("Runtime stdio request timed out")
                    try: offset+=os.write(self._process.stdin.fileno(),frame[offset:])
                    except BlockingIOError: continue
                value=self._responses.get(timeout=max(.001,deadline-time.monotonic()))
            except (queue.Empty,TimeoutError):
                self.close()
                raise TimeoutError("Runtime stdio request timed out") from None
            if isinstance(value,Exception): raise value
            return decode_response(value,request_id)
        finally:self._lock.release()

    def close(self):
        if self._process.poll() is None:
            self._process.terminate()
            try: self._process.wait(timeout=2)
            except subprocess.TimeoutExpired:
                self._process.kill(); self._process.wait(timeout=2)
        for pipe in (self._process.stdin,self._process.stdout):
            if pipe: pipe.close()

class Client:
    def __init__(self, transport: Transport): self.transport=transport
    @classmethod
    def http(cls,url: str,**options): return cls(HttpTransport(url,**options))
    def call(self,method: str,params: dict[str,Any] | None = None,timeout: float | None = None) -> Any: return self.transport.request(method, params or {}, timeout)
    def initialize(self) -> dict[str,Any]: return self.call("runtime/initialize")
    def schema(self) -> dict[str,Any]: return self.call("runtime/schema")
    def discover(self) -> list[dict[str,Any]]: return self.call("runtime/discover")
    def spawn(self,options: InputSpawnOptions) -> SessionSnapshot: return self.call("session/create",options)
    def prompt(self,session_id: str,prompt: str | list[InputPromptContent],idempotency_key: str) -> SubmissionReceipt: return self.call("session/prompt",{"sessionId":session_id,"idempotencyKey":idempotency_key,**({"text":prompt} if isinstance(prompt,str) else {"content":prompt})})
    def submission(self,session_id: str,idempotency_key: str) -> SubmissionReceipt | None: return self.call("session/submission",{"sessionId":session_id,"idempotencyKey":idempotency_key})
    def reconcile_submission(self,session_id: str,idempotency_key: str) -> SubmissionReceipt | None: return self.call("session/submission/reconcile",{"sessionId":session_id,"idempotencyKey":idempotency_key})
    def close(self,session_id: str): return self.call("session/close",{"sessionId":session_id})
    def events(self,session_id: str,after_sequence: int = 0,timeout: float = 600,poll_interval: float = .2,cancel: threading.Event | None = None) -> Iterator[RuntimeEvent]:
        if not math.isfinite(timeout) or not math.isfinite(poll_interval) or timeout <= 0 or poll_interval <= 0: raise ValueError("Timeout and poll interval must be positive")
        end=time.monotonic()+timeout
        while time.monotonic() < end:
            if cancel and cancel.is_set(): raise InterruptedError("Event polling cancelled")
            page=self.call("session/transcript",{"sessionId":session_id,"afterSequence":after_sequence,"limit":256},max(.001,end-time.monotonic()))
            for event in page:
                if event.get("sessionId") != session_id or event["sequence"] <= after_sequence: raise ProtocolError("Runtime event ordering or session identity mismatch")
                after_sequence=event["sequence"]
                yield event
            if len(page) < 256:
                wait=min(poll_interval,max(0,end-time.monotonic()))
                if cancel: cancel.wait(wait)
                else: time.sleep(wait)
        raise TimeoutError("Event polling deadline exceeded")
    def wait_for_turn(self,session_id: str,**options) -> list[RuntimeEvent]:
        events=[]
        for event in self.events(session_id,**options):
            events.append(event)
            if event["type"] == "turn.ended": return events
            if event["type"] == "session.ended": raise ProtocolError("Session ended before turn completion")
        return events
