# AI protocol library

`swarmie-ai` is an independent Rust library in `crates/ai`. It prepares and sends
protocol requests. The native runtime uses it through `swarmie-model-provider-ai`,
which implements the neutral provider contract and applies model submission protection.

All protocols use the same immutable preparation, admission and HTTP execution.
Each protocol has its own request checks, authentication headers and stream
terminal rules. Provider names and model text do not select a protocol.

## Supported protocol profiles

| Protocol | Required output cap | Authentication |
| --- | --- | --- |
| Responses | `max_output_tokens` | Bearer API key |
| Chat Completions, OpenAI profile | `max_completion_tokens` or `max_tokens`, exclusively | Bearer API key |
| Chat Completions, DeepSeek or Kimi profile | `max_tokens` | Bearer API key |
| Anthropic Messages | `max_tokens` | `x-api-key`, with `anthropic-version: 2023-06-01` |
| Gemini GenerateContent | `generationConfig.maxOutputTokens` | `x-goog-api-key` |
| Ollama native chat | `options.num_predict` | Explicit API key or anonymous access |

`Protocol::ChatCompletions` takes an explicit `ChatProfile`. Compatible profiles
have distinct Catalog adapter identities. DeepSeek and Kimi reasoning fields keep
their original values. A compatible profile cannot inherit another profile's
metadata qualification.

The library uses `async-openai` 0.42.2 with only `response-types` and
`chat-completion-types`. The SDK checks known request shapes. The library does not
use the SDK client, its request serialization, retries or credential configuration.
Native protocols check their supported envelopes directly and retain protocol JSON.
The explicit protocol-JSON path preserves those envelopes without a neutral-message conversion. Typed completions use the shared codec records below.

## Explicit credential sources

`Model::from_source` validates the captured route and metadata before asking a
`CredentialSource` for the selected account's credentials. Its future may remain
on a local account actor. The returned `Credentials` must name the exact model,
adapter, endpoint, deployment and revision. Resolution finishes before request
preparation; admission and dispatch do not select an account or renew tokens.
`StaticCredentials` supports explicitly supplied material without environment
lookup. A source failure has a `NotDispatched` effect and never triggers fallback.

Hosts that inspect final content before credential access can use `Model::deferred`
with `AuthProfile::ApiKey`. Preparation and admission freeze the request without a
secret. `Admitted::authenticate` then attaches credentials for that exact identity.
It does not change the body, destination or fingerprint. Dispatch refuses missing,
foreign or expired credentials. Authentication does not select or renew an account.

`Admitted::dispatch_local` uses a supplied local executor. Its final authority
check can retain local host state. The host must keep the execution alive through
settlement. `Execution::stop_handle` signals local cancellation without granting
permission to repeat the request.

The optional `ai` feature of `swarmie-provider-auth` exposes
`ChatgptCredentialSource`. It reuses the native dynamic-registration login,
loopback callback and token renewal. Initial registration uses
`dynamic_agent_client`; subsequent authorization and renewal use the issued
client ID associated with the verified account and persistent host. PKCE, state,
nonce, signed identity and granted direct-plan permission remain required.
The source checks the selected registration separately, captures only the access
token and expiry, and leaves refresh tokens and publication with account custody.
It refuses account substitution, expiry, legacy gateways and API-key adapter
identities. Capturing these credentials does not qualify a model or enable
ChatGPT inference: `AuthProfile::ChatGpt` and `LegacyCodex` remain unavailable in
the operational request path until their separate contracts are qualified.

## Pure provider codecs

`codecs` exposes shared completion/native message types, usage and cost values,
provider encoding/decoding, stream events and document folds. These interfaces
perform no dispatch and grant no account access. They retain native items and
signatures alongside their typed projections. Missing usage counters remain
unknown; caller-supplied price arithmetic is not a provider billing receipt.
Captured encoder facts do not replace the independent Catalog or its one
operational cache.

`Model::prepare` accepts `CompletionRequest` through the same prepared-request
path as explicit protocol JSON. Typed input requires an exact captured model,
an explicit output ceiling and already valid tool-call/result pairs. Documents
and history that require implicit repair or projection refuse before dispatch.
Native items from matching provider replies retain their fingerprints, arguments,
signatures and extension fields. Invalid Gemini signatures refuse continuation;
the encoder never removes them or manufactures a signature placeholder.
Provider options that silently omit a requested output schema also refuse.
The optional `telemetry` feature enables instrumentation; request content still
requires its explicit recording flag. Diagnostic tracing never records raw
protocol JSON. Credentials redact serialization/debug output and erase their
owned string allocations when dropped.

Typed request instrumentation starts at the physical attempt after admission and
stays active through response decoding. The decoder reuses that span, preserves
the original wire mode and records reported usage and transport request IDs.
The host supplies the tracing subscriber; the library does not configure an
exporter or send telemetry on its own.

## Prepare and admit a request

Construct `Model` with an existing `Arc<Snapshot>`, an explicit `Protocol` and
`AuthProfile`, the selected optional credential, its optional expiry and `Timeouts`.
The snapshot comes from the independent Model catalog. Its adapter identity must
equal `Protocol::adapter()`. Metadata for another protocol or profile is refused.

For Gemini, the snapshot endpoint is a model resource, such as
`https://provider.example/v1beta/models/chosen-model`. Preparation selects the
`generateContent` or `streamGenerateContent?alt=sse` method. The model resource must
match the snapshot model. For other protocols, the endpoint is the full POST URL.
Credentials must not appear in the URL. HTTPS is required except for loopback HTTP
fixtures. Construction performs no discovery, account selection or refresh.

Call `Model::prepare` with an operation UUID, a `CompletionRequest` or protocol JSON, and `WireMode`. The
request must use the captured model and a positive output cap within its known
limit. Streaming and tools require matching capabilities in the snapshot. Only
custom function tools are supported. Remote conversation references, hidden cached
history and background execution are refused. Responses image-generation tools
and explicit image/audio output modalities are refused. Responses sets `store: false` and disables automatic truncation. Chat streams
request usage explicitly. Chat and Gemini accept one output candidate. An explicit
Ollama `num_ctx` must match the captured context limit; it cannot silently change
the context behind admission.

Preparation sets explicit wire fields and serializes the final body once. Typed
input uses the codec's owned bytes; read-only protocol guards cannot rewrite
them. Authentication, content type, accepted response format and provider headers
are captured before admission. Preparation retains unknown fields on accepted records. Unknown tagged SDK input variants and
unsupported native content types refuse explicitly. The library does not repair,
reorder or normalize history. Reasoning signatures, redacted thinking, namespaced
tool fields and tool argument values retain their original JSON representation.

`Prepared::description` exposes the operation, protocol, final destination, model
identity, metadata generation, wire mode, output reservation, byte count and
fingerprint. It exposes no payload or credential. The fingerprint binds the
protocol profile, operation, final destination, generation, body and public header
representation. Sensitive header values use a fixed marker in this public digest;
the actual credentials remain private in the frozen request. It is an
inspection value and does not grant dispatch authority.

Call `Prepared::admit` with the embedding application's policy. This callback can
inspect the exact final bytes, private headers and retained snapshot. It must measure input,
check input and context limits, reserve output, and apply submission protection.
The library does not supply a tokenizer or treat byte counts as token counts.
The returned `Admitted` value owns the same private body and headers. It cannot be cloned,
serialized or reconstructed from a description.

## Execute and observe

Record dispatch intent before calling `Admitted::dispatch`. Supply a Tokio runtime
handle and a local authority guard. The guard checks captured authority immediately
before transport starts. It must not select an account, refresh credentials or do
network discovery. The library also checks snapshot and credential expiry before
admission, dispatch and transport startup.

The library sends the admitted bytes to the captured destination. It disables HTTP
retries, redirects and environment proxy selection. Complete and streaming calls
use the same implementation. `WireMode` is independent of progress observation.

The host retains `Execution` and always collects its terminal outcome. Presentation
receives only bounded `Execution::previews` subscriptions. Previews are not tool
invocations or final results. Dropping preview receivers does not stop decoding or
prevent result delivery to the host. Dropping `Execution` leaves transport running
but loses result delivery; the handle alone is not a durable receipt store. Keep
the runtime alive through local physical settlement. `Execution::stop` requests
local transport termination. `Execution::wait` returns an `Outcome` or a typed
failure. Local termination does not establish remote cancellation.

A complete call has a total timeout. Streams have separate response-header and
idle timeouts. `max_response_bytes` bounds the received body or event stream.
SSE accepts LF, CRLF and CR line endings and a leading byte-order mark. Invalid
UTF-8 fails explicitly. SSE events require a blank line. NDJSON permits a final
line without a line ending. Event fields cannot cause an automatic reconnection.

| Stream protocol | Required terminal evidence |
| --- | --- |
| Responses | A completed or incomplete response with usage |
| Chat Completions | A finished choice, reported usage and `[DONE]` |
| Anthropic | Ordered block closure, a stop reason, reported usage and `message_stop` |
| Gemini | A finished candidate, reported usage and clean EOF |
| Ollama | A native `done: true` record with reported counts |

Premature EOF, conflicting identities, open Anthropic blocks and missing required
usage fail explicitly. A confirmed terminal is preserved if trailing transport
framing is malformed. Gemini requires EOF because its stream has no separate
whole-response terminal event.

Typed preparation also exposes `Outcome::completion()` and `Outcome::usage()`.
Reported usage uses the provider's counter semantics, including reasoning and
cache components. A counting preflight and a catalog-priced estimate remain
separate from reported usage and an actual bill. `provider_request_id()` retains
the transport request ID when supplied. Unknown finish reasons retain an explicit
error stop. A projection failure keeps the confirmed raw outcome available and
never grants replay. Tool execution and successful-turn publication require the
host's own validation of the completion and its stop reason.

`Outcome::response()` returns complete protocol JSON. A streamed Responses call
uses its terminal response. Other streams reconstruct their protocol document
after the required terminal evidence. Text and reasoning fragments join. Tool
arguments join by call identity. Anthropic parses argument fragments into an
input object and refuses malformed JSON. Gemini retains separate signed parts.
Ollama retains message fragments when its final record has empty text.

`Outcome::events()` retains the original stream JSON in arrival order, including
opaque signatures, argument fragments, usage details and unknown fields. These
events remain the source for fields whose stream meaning is unknown. Document
reconstruction does not grant tool authority or make truncated arguments valid.
The caller must validate the result before history publication or tool execution.
Text previews do not replace the final result.

## Ownership and limits

The shared model catalog is available as `swarmie_ai::catalog`. It exposes exact
model descriptions, feature checks, reviewed overlays and provider listing parsers.
It reuses the standalone metadata crate and its existing operational snapshot
cache. See [Model catalog](plugins/model-catalog.md) for source precedence and
provenance. Descriptive support does not replace the qualified snapshot used by
`Model`, admission or account authorization.

Failures before transport have `Effect::NotDispatched`. Transport failures, HTTP
errors, Stop after transport starts and runtime loss have `Effect::Unknown`.
Unknown does not permit replay, another account, or fallback to another adapter.
The library has no recovery registry or durable operation store. The application
owns dispatch intent, result retention, durable recognition and restart policy.

`AuthProfile::ApiKey` is enabled for all listed protocols. `Anonymous` is enabled
only for native Ollama. `ChatGpt` and `LegacyCodex` refuse before network activity.
API-key fixtures do not establish OAuth entitlement, real account metadata or
image-generation access. The library has no account store, custody, login, refresh,
Tembra adapter or second Catalog cache.

Run `just ai-check` for behavior tests, strict Clippy and Rustdoc checks. The tests
use synthetic metadata and local HTTP fixtures. They do not perform paid inference
or qualify a real account. Run `just rust-fmt-check` for source formatting.
