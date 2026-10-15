# Process transport

`JsonProcess::spawn` keeps the default JSONL transport behavior. Use
`spawn_with_env(executable, args, cwd, style, &environment)` to override child
environment values without changing the parent process. Environment names must
be nonempty and contain neither `=` nor NUL; values cannot contain NUL. Other
environment variables remain inherited.

`spawn_with_options` accepts `ProcessOptions` with optional recording and stall
diagnostics. Both are disabled by default.

```rust,no_run
use process::{JsonProcess, ProcessOptions, WireRecording, WireStyle};
use std::{collections::BTreeMap, path::Path, time::Duration};

# async fn example() -> swarmie::Result<()> {
let options = ProcessOptions {
    environment: BTreeMap::new(),
    recording: Some(WireRecording {
        path: "/tmp/fresh-process-wire.jsonl".into(),
        max_bytes: 4 * 1024 * 1024,
    }),
    stall_timeout: Some(Duration::from_secs(120)),
};
let (wire, mut frames) = JsonProcess::spawn_with_options(
    Path::new("harness"), &[], Path::new("."), WireStyle::JsonRpc, &options,
).await?;
wire.set_active(true);
// The adapter consumes frames and clears activity at the native turn boundary.
wire.set_active(false);
wire.close().await?;
# Ok(())
# }
```

Recording creates a fresh file and never overwrites an existing file. Its parent
directory must already exist. Files use mode `0600` on Unix. Each JSONL row has
`direction` (`send` or `receive`), `timestampMs`, and `frame`. Sending is recorded
before the write attempt; receiving is recorded before response dispatch.
Arguments, environment maps, stderr, and synthetic process events are not
recorded. Credential fields, headers, environment and configuration objects are
redacted recursively. Known environment values are also removed from echoed
strings. This is structured redaction: an unknown credential placed in arbitrary
unstructured text may still be present, so recordings remain private debug data.

The byte budget must be between 1 byte and 128 MiB. Reaching the budget stops
recording and emits `wire_recording_full`; a write failure stops recording and
emits `wire_recording_failed`. These warnings leave the process running. Invalid
recording options and file-open errors are explicit spawn errors.

Stall diagnostics require an outstanding native request or an explicitly active
operation. An adapter should call `set_active(true)` when a streamed operation
starts and `set_active(false)` at its terminal boundary, including failure and
cancellation. With no request or active operation, an idle process never emits
a stall warning. A warning is emitted once per quiet period; subsequent wire
activity rearms it. The timeout must be at least 10 ms. Diagnostics use the raw
frame shape `{"_runtime_diagnostic":{"level":"warning","code":"…","message":"…"}}`;
adapters normalize them into ordinary diagnostic events. They do not kill the
process or fail the session.

Executable discovery checks explicit paths, current PATH, and known locations
before a login-shell PATH fallback. It never launches a harness. The shell
command is constant, lookup has a one-second process deadline and a bounded
output wait, and positive and negative results are cached for five minutes.
