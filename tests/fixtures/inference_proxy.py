#!/usr/bin/env python3
"""Local test bridge: Anthropic Messages to an OpenAI-compatible model endpoint.

Secrets come only from environment variables. This fixture never records headers,
prompts or responses. It is not part of the runtime or a production API gateway.
"""
import argparse
import json
import os
import time
import urllib.error
import urllib.request
import uuid
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer


def messages(body):
    result = []
    system = body.get("system")
    if system:
        result.append({"role": "system", "content": system if isinstance(system, str) else "\n".join(b.get("text", "") for b in system)})
    for message in body.get("messages", []):
        content = message.get("content", "")
        if isinstance(content, str):
            result.append({"role": message["role"], "content": content})
            continue
        text = []
        calls = []
        tool_results = []
        for block in content:
            kind = block.get("type")
            if kind == "text":
                text.append(block.get("text", ""))
            elif kind == "tool_use":
                calls.append({"id": block["id"], "type": "function", "function": {"name": block["name"], "arguments": json.dumps(block.get("input", {}))}})
            elif kind == "tool_result":
                value = block.get("content", "")
                if isinstance(value, list):
                    value = "\n".join(b.get("text", "") for b in value)
                tool_results.append({"role": "tool", "tool_call_id": block["tool_use_id"], "content": value})
            elif kind not in ("thinking", "redacted_thinking"):
                raise ValueError("fixture supports text and tools only")
        if text or calls:
            value = {"role": message["role"], "content": "\n".join(text)}
            if calls:
                value["tool_calls"] = calls
            result.append(value)
        result.extend(tool_results)
    return result


class Handler(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def log_message(self, *_):
        pass

    def upstream(self, path, body=None):
        data = None if body is None else json.dumps(body).encode()
        request = urllib.request.Request(self.server.upstream + path, data=data, headers={"Authorization": "Bearer " + self.server.api_key, "Content-Type": "application/json"})
        return urllib.request.urlopen(request, timeout=180)

    def json_reply(self, status, value):
        encoded = json.dumps(value).encode()
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(encoded)))
        self.end_headers()
        self.wfile.write(encoded)

    def do_GET(self):
        try:
            with self.upstream("/v1/models") as r:
                self.json_reply(200, json.load(r))
        except Exception as error:
            self.json_reply(502, {"error": {"type": "api_error", "message": str(error)}})

    def do_POST(self):
        try:
            length = int(self.headers.get("Content-Length", "0"))
            if length > 8 * 1024 * 1024:
                self.json_reply(413, {"error": {"message": "fixture input too large"}})
                return
            body = json.loads(self.rfile.read(length))
            path = self.path.split("?", 1)[0]
            if path.endswith("/messages/count_tokens"):
                self.json_reply(200, {"input_tokens": max(1, len(json.dumps(body)) // 4)})
                return
            if path.endswith("/messages"):
                self.anthropic(body)
                return
            if path.endswith("/chat/completions") or path.endswith("/responses"):
                body["model"] = self.server.model
                if "max_tokens" in body:
                    body["max_tokens"] = min(body["max_tokens"], 2048)
                if "max_output_tokens" in body:
                    body["max_output_tokens"] = min(body["max_output_tokens"], 2048)
                with self.upstream("/v1/" + ("responses" if path.endswith("/responses") else "chat/completions"), body) as response:
                    if body.get("stream"):
                        self.send_response(200)
                        self.send_header("Content-Type", "text/event-stream")
                        self.send_header("Connection", "close")
                        self.end_headers()
                        for line in response:
                            self.wfile.write(line)
                            self.wfile.flush()
                        self.close_connection = True
                    else:
                        self.json_reply(200, json.load(response))
                return
            self.json_reply(404, {"error": {"message": "unsupported fixture endpoint"}})
        except (BrokenPipeError, ConnectionResetError):
            pass
        except Exception as error:
            # HTTP errors omit request headers and credentials.
            self.json_reply(502, {"error": {"type": "api_error", "message": str(error)}})

    def anthropic(self, body):
        request = {"model": self.server.model, "messages": messages(body), "max_tokens": min(body.get("max_tokens", 1024), 2048), "stream": bool(body.get("stream"))}
        if body.get("tools"):
            request["tools"] = [{"type": "function", "function": {"name": t["name"], "description": t.get("description", ""), "parameters": t.get("input_schema", {})}} for t in body["tools"]]
        identifier = "msg_" + uuid.uuid4().hex
        base = {"id": identifier, "type": "message", "role": "assistant", "model": body.get("model", self.server.model), "content": [], "stop_reason": None, "stop_sequence": None, "usage": {"input_tokens": 0, "output_tokens": 0}}
        with self.upstream("/v1/chat/completions", request) as response:
            if not request["stream"]:
                value = json.load(response)
                message = value["choices"][0]["message"]
                if message.get("content"):
                    base["content"].append({"type": "text", "text": message["content"]})
                for t in message.get("tool_calls", []):
                    base["content"].append({"type": "tool_use", "id": t["id"], "name": t["function"]["name"], "input": json.loads(t["function"]["arguments"])})
                base["stop_reason"] = "tool_use" if message.get("tool_calls") else "end_turn"
                usage = value.get("usage", {})
                base["usage"] = {"input_tokens": usage.get("prompt_tokens", 0), "output_tokens": usage.get("completion_tokens", 0)}
                self.json_reply(200, base)
                return
            self.send_response(200)
            self.send_header("Content-Type", "text/event-stream")
            self.send_header("Connection", "close")
            self.end_headers()

            def emit(kind, **fields):
                record = dict(type=kind, **fields)
                self.wfile.write(("event: " + kind + "\ndata: " + json.dumps(record) + "\n\n").encode())
                self.wfile.flush()

            emit("message_start", message=base)
            text_started = False
            blocks = {}
            reason = "end_turn"
            output_tokens = 0
            for line in response:
                if not line.startswith(b"data:"):
                    continue
                data = line[5:].strip()
                if data == b"[DONE]":
                    break
                chunk = json.loads(data)
                output_tokens = chunk.get("usage", {}).get("completion_tokens", output_tokens)
                for choice in chunk.get("choices", []):
                    delta = choice.get("delta", {})
                    if delta.get("content"):
                        if not text_started:
                            emit("content_block_start", index=0, content_block={"type": "text", "text": ""})
                            text_started = True
                        emit("content_block_delta", index=0, delta={"type": "text_delta", "text": delta["content"]})
                    for call in delta.get("tool_calls", []):
                        index = call["index"] + 1
                        function = call.get("function", {})
                        if index not in blocks:
                            blocks[index] = True
                            emit("content_block_start", index=index, content_block={"type": "tool_use", "id": call.get("id", "tool_" + uuid.uuid4().hex), "name": function.get("name", ""), "input": {}})
                        if function.get("arguments"):
                            emit("content_block_delta", index=index, delta={"type": "input_json_delta", "partial_json": function["arguments"]})
                    if choice.get("finish_reason") == "tool_calls":
                        reason = "tool_use"
                    elif choice.get("finish_reason") == "length":
                        reason = "max_tokens"
            if text_started:
                emit("content_block_stop", index=0)
            for index in blocks:
                emit("content_block_stop", index=index)
            emit("message_delta", delta={"stop_reason": reason, "stop_sequence": None}, usage={"output_tokens": output_tokens})
            emit("message_stop")
            self.close_connection = True


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--port", type=int, default=8092)
    parser.add_argument("--upstream", default=os.environ.get("SWARMIE_FIXTURE_BASE_URL", "http://127.0.0.1:8091"))
    parser.add_argument("--model", default=os.environ.get("SWARMIE_FIXTURE_MODEL", "fixture-model"))
    args = parser.parse_args()
    key = os.environ.get("SWARMIE_FIXTURE_API_KEY")
    if not key:
        parser.error("SWARMIE_FIXTURE_API_KEY is required")
    server = ThreadingHTTPServer(("127.0.0.1", args.port), Handler)
    server.upstream = args.upstream.rstrip("/")
    server.api_key = key
    server.model = args.model
    print(f"Fixture inference proxy listening on http://127.0.0.1:{server.server_port}", flush=True)
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        pass
    finally:
        server.server_close()


if __name__ == "__main__":
    main()
