#!/usr/bin/env python3
"""Linux/Unix SSH command fixture; it does not qualify actual SSH authentication."""
import json
import os
import sys

with open(os.environ["SWARMIE_SSH_FIXTURE_LOG"], "a", encoding="utf-8") as ledger:
    ledger.write(json.dumps({"pid": os.getpid(), "command": sys.argv[-1]}) + "\n")
if sys.argv[-1].endswith('"$SWARMIE" --version') or sys.argv[-1].startswith("command -v swarmie"):
    if os.environ.get("SWARMIE_SSH_FIXTURE_INSTALLED") == "0":
        sys.exit(127)
    print(os.environ.get("SWARMIE_SSH_FIXTURE_VERSION", "swarmie 0.1.0"))
    sys.exit(int(os.environ.get("SWARMIE_SSH_FIXTURE_EXIT", "0")))
if os.environ.get("SWARMIE_SSH_FIXTURE_BAD_PROTOCOL") == "1":
    import base64, hashlib
    headers = {}
    while True:
        line = sys.stdin.buffer.readline().strip()
        if not line:
            break
        if b":" in line:
            key, value = line.split(b":", 1)
            headers[key.lower()] = value.strip()
    accept = base64.b64encode(hashlib.sha1(headers[b"sec-websocket-key"] + b"258EAFA5-E914-47DA-95CA-C5AB0DC85B11").digest())
    sys.stdout.buffer.write(b"HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: " + accept + b"\r\n\r\n")
    response = json.dumps({"jsonrpc":"2.0", "id":0, "result":{"name":"swarmie", "protocolVersion":"unsupported"}}).encode()
    sys.stdout.buffer.write(bytes([0x81, len(response)]) + response)
    sys.stdout.buffer.flush()
    sys.stdin.buffer.read()
    sys.exit(0)
os.execv("/bin/sh", ["sh", "-c", sys.argv[-1]])
