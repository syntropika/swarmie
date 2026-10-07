# Python runtime client

Install from this directory or a local wheel:

```sh
python3 -m pip install /absolute/path/packages/clients/python
```

```python
from mpai_client import Client, RpcError

client = Client.http("http://127.0.0.1:7457")
client.initialize()
session = client.spawn({"harness": "codex", "cwd": "/absolute/project"})
try:
    client.prompt(session["id"], "Describe the project.")
    for event in client.events(session["id"], after_sequence=session["sequence"]):
        if event["type"] == "text.delta":
            print(event["data"]["text"], end="", flush=True)
        if event["type"] == "turn.ended":
            break
finally:
    client.close(session["id"])
```

Pass `token` to `Client.http` when required. `call(method, params)` exposes every RPC method. Generated `TypedDict` options and snapshots live in `mpai_client.generated`; provider variants stay flexible dictionaries. Image content uses `{"type":"image","data":{"mime_type":"image/png","data":"BASE64"}}` and `prompt` accepts a list of content blocks.

`RpcError.code` and `.data` preserve structured server errors. HTTP requests default to a 30-second deadline and a 16 MiB response limit. Event iteration defaults to a ten-minute deadline, 200 ms polling interval, and 256-event pages. Pass a `threading.Event` as `cancel` to stop between requests; an in-flight request is bounded by its timeout.

Unix clients may use `Client(StdioTransport(["/absolute/path/mpai", "serve"]))`. Close the transport when finished. Serialized pipe writes and reads have deadlines; timeout terminates the owned process. Stdio is verified on Linux; Windows pipe polling is not supported. WebSocket and Unix socket transports are not provided. The package has no external runtime dependencies and is not claimed to be published on PyPI.
