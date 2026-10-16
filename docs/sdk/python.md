# Python runtime client

Run from the repository root to install the Python SDK or a local wheel:

```sh
just sdk-build
python3 -m pip install /absolute/path/sdk/python
```

```python
import uuid
from swarmie_client import Client, RpcError

client = Client.http("http://127.0.0.1:7457")
client.initialize()
session = client.spawn({"harness": "codex", "cwd": "/absolute/project"})
try:
    key = str(uuid.uuid4())  # Persist this key before sending.
    client.prompt(session["id"], "Describe the project.", key)
    for event in client.events(session["id"], after_sequence=session["sequence"]):
        if event["type"] == "text.delta":
            print(event["data"]["text"], end="", flush=True)
        if event["type"] == "turn.ended":
            break
finally:
    client.close(session["id"])
```

Pass `token` to `Client.http` when required. `call(method, params)` exposes every RPC method. Generated `TypedDict` options and snapshots live in `swarmie_client.generated`; provider variants stay flexible dictionaries. Image content uses `{"type":"image","data":{"mime_type":"image/png","data":"BASE64"}}` and `prompt` accepts a list of content blocks.

`RpcError.code` and `.data` preserve structured server errors. HTTP requests default to a 30-second deadline and a 16 MiB response limit. Event iteration defaults to a ten-minute deadline, 200 ms polling interval, and 256-event pages. Pass a `threading.Event` as `cancel` to stop between requests; an in-flight request is bounded by its timeout.

Desktop clients may use `Client(StdioTransport(["/absolute/path/swarmie", "serve"]))`. Close the transport when finished. Rust owns serialized pipe writes and bounded reads; timeout or cancellation terminates the owned process. Pass `cancel=threading.Event()` to calls, events, or turn collection to cancel active I/O. Use `client.dispose()` or a `with Client.http(...) as client:` block to release native resources. Linux and macOS libraries are built per host; Windows qualification remains pending. WebSocket and Unix socket transports are not provided. The package loads its bundled Rust library through ctypes without a Python extension ABI dependency and is not claimed to be published on PyPI.

Each intentional prompt requires its own persisted idempotency key. Reuse the original key and content to obtain the retained receipt without dispatching again. Query `session/submission` after a lost response; this read does not open a native session. `delivery` distinguishes `not_sent`, `in_flight`, `accepted`, and `unknown`. Acceptance does not mean completion. An uncertain receipt blocks new work; clients do not automatically resend. Keys are scoped to a personal node session and survive event-history trimming while its submission ledger is retained.

Use `session/submission/reconcile` (or the client reconciliation helper) to check an already open native connection without resending work. Explicitly resume a closed session first. Native absence is not rejection; partial evidence and unsupported adapters keep uncertain work paused. Settled receipts require no native lookup. Check `capabilities.submissionReconciliation` for qualified lookup support.
