# Model inference integration

Swarmie owns the model vocabulary in `crates/model-contracts`. The compiled plugin in
`plugins/model-provider-genai` translates these DTOs into genai requests and observations.
Consumers depend on Tembra and the neutral crate. Inference has no Session Store, Durable
Tasks, conversation persistence, tool execution or Agent Loop dependency.

## Discovery and selection

Bind `model_providers()` through the existing Host or Facet scope. Each Registry entry is a
`ProviderRegistration` with an opaque provider ID, supported contract version, independently
named provider/generation Contracts and configured connection IDs. Registry entries live only
as long as their provider object; a cached observation does not authorize future calls.

Construct `ModelEndpoint::new(registration.provider_contract)` and
`GenerationEndpoint::new(registration.generation_contract)`. These are ordinary Tembra
`ContractToken`s with the exact canonical schema under each provider's distinct identity.
Tembra checks the complete descriptor fingerprint when binding. Other libraries can implement
`ModelProviderSchemaProvider` and `ModelGenerationSchemaProvider` at independently named
endpoints and register their own metadata. Several adapters can coexist without modifying
consumers or introducing a global provider catalog.

A `ModelIdentity` names a connection and model relative to the selected provider endpoint.
Connection IDs may repeat across different providers; retain the Registry selection alongside
the identity. Refuse ambiguous selection instead of choosing the first matching connection.
A provider's `models(connection)` operation discovers remote candidates when supported;
configured model admission remains authoritative. Discovery is bounded to 1024 names, with a
payload-limit error rather than a truncated complete-looking result if that bound is exceeded.

The runtime's compiled catalog exposes the provider as an optional Core Plugin. An application
Profile selects it with explicit configuration. It is not enabled by the default foundation.
A native configuration change requires a fresh Host; compatible JavaScript reload is not a
native provider replacement mechanism.

## Requests and messages

`ModelRequest` contains a fresh caller request ID, model identity, ordered `Message`s, tool
schemas and generation options. `Message` and `ContentPart` are inference DTOs. Session owners
can embed these parts in historical event envelopes, adding event identity, causal ordering and
presentation metadata without defining another inference message hierarchy.

Roles are system, developer, user, assistant and tool. The maintained genai patch maps
`Developer` natively to OpenAI Chat Completions and Responses input messages. Simultaneous
system/developer messages retain their literal roles and request order. Responses does not
hoist them into top-level `instructions`, which has different per-call replacement semantics.
OpenAI model-specific instruction precedence remains the remote API's behavior; the adapter
does not emulate it. `developer_role` is `ModelDependent` for these two adapters. Other
adapters reject developer messages with `Unsupported`/`NotSent`; there is no implicit
Developer-to-System or Developer-to-User translation. An explicit caller transformation must
happen before inference, with its changed semantics owned by that caller.
Text, reasoning, images, tool calls/results and versioned extensions are separate content parts.
Final tool arguments must be JSON objects and call IDs remain byte-for-byte identical between
assistant calls and tool results. Tool names in results are recovered from the preceding
assistant calls; missing, duplicated or unresolved call IDs are refused before submission.

`ToolResult.is_error` is carried as a JSON object containing `is_error: true` and `value` in the
provider's tool-result text. genai lacks a portable native tool-error flag; consumers must not
assume the provider receives a protocol-specific error boolean. Successful tool values retain
their ordinary JSON encoding.

Images use a MIME type and an explicit URL or base64 source. No adapter reads a caller's local
files implicitly. Provider and model support determine whether the remote endpoint accepts a
particular image. Other modalities are outside this adapter's current public content profile.

Response format supports JSON mode and named JSON Schema. JSON output is parsed locally;
provider-side schema enforcement and schema dialect conversion depend on the adapter/model.
This is not a general local JSON Schema validator. Tool parameter schemas require an object
root. Unknown or unsupported requested features fail explicitly before inference. Structured
answer validation applies to completed answer text. A genuine tool-call branch can contain
no answer JSON, including when it carries ordinary commentary; tool IDs and structured
arguments remain authoritative. Length, filtering and explicit provider refusal finish reasons
retain their original meaning instead of being replaced by a misleading JSON parsing failure.

| Adapter | Developer | JSON mode | JSON Schema | Tools with structured output |
| --- | --- | --- | --- | --- |
| OpenAI Chat Completions | Native, model dependent | Model dependent | Model dependent | Qualified conversion path; remote model must support it |
| OpenAI Responses | Native ordered input, model dependent | Model dependent | Model dependent | Qualified conversion path; remote model must support it |
| Anthropic | Explicit refusal | Refused: ignored by upstream | Model dependent | JSON Schema path qualified; plain JSON refused |
| Gemini generateContent | Explicit refusal | Refused: ignored by upstream | Model dependent | Refused before HTTP; this adapter does not use Interactions |
| Native Ollama | Explicit refusal | Model dependent | Refused: ignored by upstream | Refused before HTTP |
| Other genai adapters | Explicit refusal | Refused | Refused | Refused |

Capabilities describe individual paths, not every Cartesian combination. Preflight also checks
effective defaults and feature combinations. Local HTTP fixtures qualify translation and result
handling; they do not establish live commercial model availability or schema dialect support.

## Streaming through Tembra

There is no async-return-value streaming assumption or secondary subscription transport.
Acquire a referenced generation with `GenerationInput { request }` using the discovered
`GenerationEndpoint`. Acquisition validates and prepares it without sending HTTP. Hold that
Reference while subscribing to its `progress()` State, then call `start()`. Repeating `start()`
on the same live object does not execute another request.

The State has a prepared/running/settled execution phase, ordered `SequencedEvent`s, an
acknowledged cursor, final response/error and a separate `physically_settled` observation.
Text/reasoning deltas append to their logical content. `ToolCallSnapshot.arguments_json`
**replaces** the previously observed partial arguments for that call; snapshots are not JSON
fragments to concatenate. The final response holds structured arguments, finish reason, usage
and authoritative content block ordering.

One logical consumer owns acknowledgement, even when it clones a Reference for rendering.
Process each event sequence once and acknowledge only the contiguous processed prefix, in
batches. Acknowledgement is monotonic and idempotent; future cursors are refused. State
subscriptions may coalesce/reset publications, so do not count callback invocations as events.
Unacknowledged events remain in the bounded window until processed. A slow consumer stops
upstream polling when the count/byte window is full. The total deadline includes this wait.

Events are batched by count, size and timer. No per-token RPC is required. Transport pings and
genai's synthetic Start are not content events or evidence of remote admission. Publication
failures cannot imply remote rollback. An inference or rejected incremental publication ends
with a typed failure; already adopted events remain available until acknowledged or disposal.
Terminal publication failure can coexist with a successful physically completed inference;
recover its retained response rather than submitting another inference.

The generation Resource owns a worker on the **same injected Tembra Driver** as the Host.
Its closer signals cancellation and awaits the completion receipt before releasing clients.
Only short Facet-scope roots publish State; no root or consumer Reference spans network I/O.
This avoids a long managed task retaining its own referenced object's lifetime in the pinned
runtime. No second executor, EventEmitter or streaming wire protocol is introduced.

`cancel()` requests local cancellation; wait for `physically_settled` in `progress()` or
`recover()` to observe local I/O closure. Dropping the last consumer Reference closes its Resource and joins local work.
After release or Host shutdown its weak State becomes unavailable; do not wait for a final
State from a disposed reference. Host closure joins cleanup. Physical settlement describes
the local HTTP future/stream only: a remote provider may still compute or charge for the call.

`recover()` returns a bounded volatile snapshot of the same live generation. It performs no
State publication, inference or request retry. After local I/O ends, the worker retains its
terminal response/error before trying to publish Settled. Terminal publication repair makes
at most three attempts, then stops. The retained result remains readable even if an internal
projection rejects every terminal candidate; progress State can remain Running. Consumers
must use recover when State delivery fails or cancellation cleanup needs an independent
settlement observation. Recovery remains an ordinary portable Tembra operation, not another
stream transport or a durable receipt.

An internal derivation can reject an incremental candidate before adoption. That ends local
inference with `Publication`, without inventing event sequences for rejected content. Native
subscriber Result errors happen after source adoption and are reported independently by
Tembra. They cannot roll it back. A native observer panic can interrupt delivery after adoption;
the adapter checks adopted sequence/terminal state before a publication-only retry. This is
limited to unwind-capable publication boundaries, not process-abort or arbitrary panic containment. An external projection can
fail independently of an adopted source. Subscribers are responsible for rebuilding their
projection from a current snapshot. The adapter does not rerun HTTP to repair these failures.
Concurrent cancellation preserves whichever inference result has already physically settled;
Host shutdown and last-Reference disposal cancel and join the worker before releasing its
connection admission. A disposed object has no recoverable result, final notification or
surviving completion guarantee. Preserve necessary evidence in the caller's own lifecycle.

Reconnect by acquiring from a fresh Host and making an explicit new attempt. No generation
history or idempotency receipt survives object disposal. A reused request ID on a new object
can cause another remote inference. The caller must own any durable reconciliation policy.

## Results and continuation

`GenerateResult` distinguishes a `ModelResponse` from a sanitized `ProviderError`. The response
contains the caller request ID, requested model identity, assistant message, normalized and raw
finish reasons, token usage and provider response identity when supplied. Unknown finish strings
remain `Other` with their original value; missing reasons remain `Unknown`.

Token usage fields are optional. Never interpret absence as zero. Namespaced extensions
preserve normalized detail counters and allowlisted raw provider usage objects when available,
including provider-reported cost fields. The adapter does not invent a price table, currency or
billing amount. `swarmie.reported-model` records the provider's model observation separately
from configured selection.

Opaque continuation extensions use a provider namespace and version. Version 2 replay parts
stay immediately after the visible reasoning block they represent, at its original position
among text and tool calls. Removing the separate display-oriented `Reasoning` part leaves
that extension in place. Full and display-filtered DTO round trips therefore replay one copy
of each reasoning block. Conflicting visible text and replay payloads are refused. Preserve
extension order and payloads verbatim; the former version 1 appended `reasoning_replay` array
is refused because it cannot establish the original positions.

Anthropic thinking/redacted-thinking blocks retain their native payload and opaque signatures
in positional `provider_content` extensions. Responses preserves each complete reasoning item,
including original item ID, summaries and encrypted continuation when supplied. Gemini keeps
signed thought blocks separate from tool-associated signatures; actual provider tool IDs are
required rather than accepting synthesized IDs as original. The local genai patch adds only
narrow native Custom passthrough paths for representations its normalized API cannot retain.
Already parsed raw response/frame reconstruction is bounded and provider-specific. Ambiguous
or unsupported block shapes and lossy streaming arrangements fail with DataLoss rather than
silently flattening or reordering them. Legitimately exposed reasoning remains available as
Reasoning content; opaque values are never converted to conversational text.
Unknown namespaces/versions are refused on replay rather than silently dropped.
A namespace identifies a wire representation; continuation is not portable to another provider
or necessarily another model. Keep the selected endpoint/connection with the original message.
Do not expose opaque signatures as conversational text or interpret them as credentials.

Session integration must preserve these supplied continuation parts if it promises replay
across turns/restarts. Dropping them for presentation requires a separate authoritative private
continuation representation. This provider stores none of that history. Agent Loop owns the
mapping between its Session/Task IDs and each fresh model request ID; use those IDs in its
own durable records and retain returned provider IDs when available.

## Configuration and credentials

A Profile configuration can contain:

```json
{
  "connections": [
    {
      "id": "primary",
      "provider": "openai",
      "models": ["selected-model"],
      "base_url": "https://provider.example/v1/",
      "credential_env": "MODEL_API_KEY",
      "timeout_ms": 120000,
      "max_concurrent": 4,
      "defaults": {"max_output_tokens": 2048}
    }
  ],
  "limits": {"batch_events": 32, "batch_interval_ms": 20}
}
```

`credential_env` is a reference, never the secret value. The default resolver reads the named
supervisor environment entry during Resource opening. Embedders can supply a private
`Credentials` implementation. Inline credential fields are rejected. Missing credentials fail
installation readiness with static diagnostics. Private clients are Resource-owned and never
published in State, Registry entries or portable DTOs.

Each configured connection has a client bound to its provider adapter; model-name guessing and
process-global provider state are not used. Base URLs may be customized, with embedded URL
credentials, query strings and fragments refused. HTTP redirects and automatic HTTP retries
are disabled. Native Ollama supports credential-free local inference and JSON mode, but its
schema output is explicitly refused because genai ignores that option. OpenAI-compatible
endpoints use the OpenAI adapter and explicit connection credentials where that adapter requires
them. Other genai adapters are selectable; advanced capabilities are conservatively refused
unless their conversion path is supported. Capability values distinguish unsupported features
from paths whose remote acceptance depends on the model.

## Failures, limits and retry ownership

Errors contain a category, submission observation, optional HTTP status and bounded numeric
retry delay. Raw vendor error strings, bodies, request payloads and endpoint URLs are never
returned or logged. The adapter isolates genai inference polling under a local empty tracing
subscriber because upstream error logs can echo response bodies; the caller's tracing context
is restored outside that future. Log only sanitized categories in consumers.

Local validation/capacity refusal is `NotSent`. Once network work may have started, timeout,
cancellation or transport failure is `MayHaveBeenSent`; an HTTP response is `ResponseReceived`.
These observations are not exactly-once or nonprocessing guarantees. There are no automatic
inference retries, durable retries or stream reconnections. Agent Loop owns retry decisions and
ambiguous-result reconciliation; a 429/503 or retry delay does not by itself authorize replay.

Limits bound request DTOs, final responses, parsed events, retained State windows, active HTTP
work per connection and live generation objects. Combined response/window budgets reserve
space below Tembra's default frame bound. A bound failure returns no false completion.
**genai 0.7.0 buffers HTTP bodies and incomplete SSE frames internally before the adapter can
inspect them. These limits do not provide a hard bound on that upstream transient memory.**
A hostile/unbounded endpoint requires an upstream bounded-body transport change or an externally
bounded inference gateway before it can be treated as memory-isolated. A concrete follow-up
must count decoded body/error-body bytes before accumulation, scan SSE/NDJSON with a bound on
incomplete frame bytes before normalization, and bound queued complete frames and total stream
bytes. Content-Length checks and timeouts alone are insufficient. Both compressed/chunked bodies
and a single chunk containing many events need qualification. This iteration does not implement
those transport limits.

The adapter disables parallel tool calls for OpenAI paths and inspects raw frames for known
loss patterns. Multiple calls in one OpenAI frame, simultaneous tool/text fields or unsupported
reasoning-detail blocks fail with `DataLoss` before publishing the affected normalized event.
Provider-specific metadata that genai never exposes remains outside the common result. Raw
frame capture is allowlisted and bounded after framing; it is not an unbounded transcript.
Malformed non-JSON frames, unsupported content-bearing events and missing Responses or
Anthropic terminal events are refused even if upstream would otherwise skip them or synthesize
a successful EOF.
Final tool IDs/names must match observed snapshots; complete argument snapshots cannot be
contradicted by the final response. An incomplete snapshot can be finished by the terminal item.

## Verification commands

Run from the repository root:

```sh
just models-check
just models-benchmark
just rust-run -p swarmie-model-contracts --bin model-contract -- provider
just rust-run -p swarmie-model-contracts --bin model-contract -- generation
```

Ordinary tests use deterministic local HTTP/SSE and a real Tembra Host. The explicit benchmark
compares State publication batching in debug mode; it is not a production throughput estimate.
`live_chat_and_streaming` is ignored by default. To run it, configure `SWARMIE_LIVE_PROVIDER`,
`SWARMIE_LIVE_MODEL`, optional `SWARMIE_LIVE_BASE_URL` and optional
`SWARMIE_LIVE_CREDENTIAL_ENV` (the name of a separate credential entry), then run:

```sh
just rust-test -p swarmie-model-provider-genai --test live -- --ignored
```

That opt-in test performs two remote inferences and may incur charges. Ordinary acceptance and
benchmarks require no commercial credentials.

## Dependency maintenance and integration changes

The adapter pins genai 0.7.0 through the root crates.io patch to the self-contained
`vendor/genai-0.7.0` source. Public model Contracts import none of its types. Original licenses
and source provenance accompany the dependency. Keep the patch scoped to native developer
serialization, explicit unsupported-role handling and narrow native continuation passthrough;
do not turn it into an independent HTTP client. When upgrading, compare the recorded upstream
source with local deltas, retain only fixes still needed, and run deterministic provider/Host
acceptance before removing or rebasing a delta. Upstream contribution is desirable; no PR or
upstream submission is implied by this local maintained patch.

The generation descriptor now includes recover() and ProviderErrorKind includes Publication.
Its exact fingerprint changes, including consumers that embed the error type. Rebuild Rust and
generated TypeScript bindings and implement recover in alternate/synthetic generation providers.
Registry contract version 1 is still the current development shape; old fingerprints do not
bind silently. Agent Loop must consume this descriptor and use recover in error/cleanup paths.
It must preserve ordered continuation extensions while filtering display reasoning. No peer
worktree or persistent session schema was modified here; combined integration remains separate.
