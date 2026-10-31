# Model catalog

`swarmie_ai::catalog` prepares model metadata for Rust consumers. This module
does not perform inference, obtain credentials or access the network. It shares
the AI library package and build-time model generation.

## Identity and preparation

`ModelIdentity` contains the provider, adapter, endpoint, deployment, model and
revision. All fields take part in identity. An absent revision means that the
revision was not reported. It does not identify a fixed provider revision.

Load exact `SourceRecord` values with `Catalog::replace_sources`. Create a
`Preparation` with the identity, configuration revision and required capabilities.
Call `Catalog::prepare` when you select the model. Keep the returned
`Arc<Snapshot>` with that operational model.

The cache has a caller-selected capacity. It reuses a valid snapshot for the same
preparation. Source replacement clears the cache. `Catalog::invalidate` removes
entries for one exact identity. Existing snapshots keep their original values.
Reading a snapshot does not fetch sources or scan the catalog.

Change the configuration revision when request configuration changes. Use a
non-secret revision. Snapshot generation is a SHA-256 digest of the preparation,
effective result and evidence. It is separate from opaque-result compatibility.

## Limits and capabilities

Limits use tokens. They do not describe Unicode input length or transport bytes.
The resolver takes the minimum admitted hard ceiling for each field. It retains
the evidence for each ceiling. Application policy cannot establish an unknown
model limit. Estimated values cannot establish an operational ceiling.

The context ceiling can provide a conservative input ceiling. Output requires an
explicit source fact. Input and output are each bounded by context. The consumer
must still reserve output and calculate occupancy of the final request.

`Metadata` distinguishes known limits from unknown limits. Default `Requirements`
requires context, input and output. Missing required limits return an error.

`Capability` distinguishes unknown, unsupported and supported operations. Only
adapter evidence qualifies an operation. Required capabilities must be supported.
The catalog does not infer support from a model name or a provider family.

Native compaction qualification specifies its method, input format, output format,
continuation format and token limits. The continuation format must identify the
opaque-state compatibility contract. Snapshot generation does not provide that
contract. The crate does not execute compaction.

## Sources and validity

Sources can declare model facts, provider restrictions, deployment limits,
configuration limits and application policy. Evidence records its source,
revision, observation time and optional expiry. Times are Unix milliseconds.

Future and expired evidence cannot establish a fresh fact. Preparation can permit
a bounded stale interval for limits. The snapshot lists stale evidence. Adapter
capabilities never use that interval. An absent expiry means that the source did
not declare an expiry; it does not claim live verification.

Validity is checked during preparation. `valid_until_ms` identifies the next
required preparation boundary. Keep the selected snapshot for an admitted
invocation. The host owns refresh before a later admission. Expiry does not
establish cancellation or retry authority for an existing invocation.

`sources::records_json` reads explicit records. `sources::ModelsDev` reads the
public models.dev API format and the build snapshot. It indexes provider and model
keys once. The embedding must confirm that the facts apply to the selected route
before binding them. Binding does not resolve aliases or infer a deployment.
External capability flags do not qualify adapter operations.

## Descriptive model facts

`swarmie_ai::catalog` owns the shared model facts and operational `Catalog` cache.
Consumers depend on `swarmie-ai` and use this module directly. There is no separate
catalog package or second cache.

`ModelsDev::model` returns an exact provider/model description. `models` iterates
in stable provider/model order. `ModelSpec` retains names, lifecycle details,
context/input/output limits, input/output modalities, tools, structured output,
reasoning controls, sampling support, attachment support, interleaving and prices.
Additional model/provider fields and pricing tiers are preserved. Prices are
descriptive USD rates per million tokens; missing prices are not zero, and this
crate does not calculate a final bill.

`ModelSpec::supports` returns `Some(true)`, `Some(false)` or `None` for a
descriptive `ModelFeature`. `require` refuses unsupported and unknown features.
An absent modality list is unknown; an explicit empty list reports no modalities.
Effort names are exact. Budget checks need explicit numeric bounds; output limits
do not supply missing bounds. Negative provider sentinels are retained and cannot
establish a token bound. Cache prices do not establish cache-retention support.
These checks do not replace exact adapter qualification or account authorization.

`ModelsDev::bundled` indexes the embedded source once per process; clones share
the immutable rows. `with_models_dev` overlays an explicitly supplied newer public
source. Unreported fields and models retain their older evidence. `with_overrides`
accepts reviewed configuration, provider or deployment patches in models.dev
provider/model shape. Supplied leaf fields are pinned across later public refreshes,
including false, zero and null. Invalid updates fail atomically and leave the
original source intact. Updates return a new source; publish its bound records
through the existing `Catalog` replacement API to invalidate prepared snapshots.

`ModelDescription::evidence` accepts a JSON pointer such as `/limit/output`.
Binding limits retains each field's evidence, including reviewed overrides.
Consumers must check the evidence's applicability and validity. Reviewed
`compatibility` fields describe wire constraints; the catalog never applies
headers, credentials, request bodies or routing automatically. Unknown compatibility
keys are rejected to catch misspelled reviewed constraints.

## Provider listings

`ModelListing::from_json` parses one OpenAI, Anthropic, Gemini or Ollama listing
page without network I/O. It retains exact IDs, original rows, the full response
and pagination fields. Gemini IDs retain the `models/` resource prefix. Optional
Gemini input/output limits and Anthropic `max_input_tokens`/`max_tokens` are exposed
when reported; zero stays unknown. A parameter maximum is not proof of a complete
generated-token ceiling. Other limits and capability fields remain in the raw
provider document without guessed mappings.

The caller owns authentication, fetching, pagination, exact-route evidence and
explicit source publication. Listing a model does not automatically merge it into
the public catalog, qualify a protocol feature or authorize inference.

## Build updates

Cargo's build script downloads the latest public metadata from
`https://models.dev/api.json`. It validates the response and writes the normalized
snapshot only to Cargo's temporary `OUT_DIR`. `include_bytes!` incorporates those
bytes into the compiled crate. There is no checked-in catalog JSON and no runtime
metadata download. The HTTPS client is a build dependency only.

The build rejects redirects, responses over 16 MiB, duplicate provider/model
keys at every depth, ambiguous model identities and invalid token limits or list
prices. A download or
validation failure fails compilation rather than silently using an old catalog.
The snapshot records exact keys and descriptive model facts, its source, source and content
digests, capture time and a seven-day freshness interval. The upstream MIT notice
is retained in `crates/ai/catalog/data/LICENSE.models-dev`.

Cargo reuses successful build outputs during incremental builds.
`just ai-catalog-build`, `just desktop-build` and `just desktop-package` set a
build refresh marker so their builds fetch metadata again. A fresh Cargo build
also downloads the catalog automatically. The marker affects build caching only.

Run `just ai-check` for resolver, source-integrity and build-validation
checks. Root foundation checks include this recipe. Compilation does not require
this guide or other documentation files.

## Native provider selection

The native provider settings prepare metadata from the same captured connection,
model and reasoning configuration used to construct the protected AI model.
The adapter checks identity and configuration before attaching the snapshot.
Metadata failure occurs before credential retrieval or provider dispatch.

OpenAI API, Anthropic API and Kimi API choices bind explicitly to their exact
catalog provider keys and configured standard endpoints. A custom endpoint or
OAuth route does not inherit another deployment's limits. It needs its own facts.
The discovery response can provide an optional `limit` object with `context`,
`input` and `output` token ceilings for each exact model. Zero and missing fields
stay unknown. Invalid fields or conflicting duplicate facts fail discovery.
These facts stay in private provider settings. Public account projections retain
only their existing model fields. A different endpoint cannot reuse these facts.

Native selection uses the embedded build snapshot as a pinned offline input.
It permits stale limit facts and exposes them in `stale_evidence`. Source updates
occur through the build process. Executable capability expiry gets no stale
extension. Local or authorized-account routes without sufficient declared facts
fail with `providers.model_metadata`; they do not receive guessed limits.

The message adapter explicitly reports native compaction as unsupported because
no native compact-endpoint contract is qualified for it. Tool and streaming
qualifications use the exact configured route, model and reasoning setting. A public catalog flag alone cannot grant adapter support.
Context selects a supported strategy using this contract.
Its final request measurement, compaction execution and activation have separate
owners.

## ChatGPT OAuth qualification remains pending

Signing in and listing an account's models do not establish usable operational
metadata. The current OAuth parser accepts optional `limit` and
`swarmie_capabilities` fields. The published
[selected-account catalog contract](https://developers.openai.com/siwc/token-sharing-open-source/models-and-inference)
does not establish that those fields exist. Synthetic servers supplying them do
not qualify the actual ChatGPT route. Without sufficient exact facts, selecting
that route returns `providers.model_metadata` before inference credential access.
This is an outstanding integration gap, not completed OAuth inference support.

Closing the gap requires an authoritative source for the exact account/model and
route's token ceilings, an explicit provenance-preserving binding in the existing
catalog, and qualification of the actual authorization profile. OAuth does not
inherit the standard API-key route's facts automatically. Unknown values must
remain unknown; model-list availability alone is not an inference entitlement.

The [ChatGPT plan preview profile](https://developers.openai.com/siwc/token-sharing-open-source/preview-limitations)
also differs from generic Responses: it requires `store: false`, `stream: true`
and an input array, and rejects `max_output_tokens` and several other fields.
Provider preparation, measurement and dispatch must qualify those constraints
together before operational support is claimed. Generic protocol fixtures alone
do not establish that compatibility.

Authenticated discovery may obtain and use its credential through the existing
authorization owner to acquire metadata. The later inference preflight consumes
the captured facts. It must not introduce a cycle that prevents discovery, another
resolver/cache in the node, or a metadata request for each inference invocation.

## Text and tool profile qualification

The native adapter qualification is `swarmie.text-tools.v1`. It requires text and
tool values that the current converter can preserve. Generic declarations do not qualify model-specific reasoning behavior. The
adapter retains native continuation records through the AI codecs. HTTP fixtures
verify full tool cycles, streaming, cancellation and retained continuations across
seven protocol profiles. These tests use synthetic servers, not paid inference.

Publisher documentation and adapter checks qualify these exact standard bindings:

| Product route | Exact models | Reasoning configuration |
| --- | --- | --- |
| OpenAI Chat Completions | `gpt-5`, `gpt-4.1` | No explicit reasoning setting |
| OpenAI Chat Completions | `gpt-5.6-terra` | Explicit `none` or `zero` |
| Anthropic Messages | `claude-sonnet-4-5`, `claude-sonnet-4-6`, `claude-opus-4-5`, `claude-opus-4-6`, `claude-haiku-4-5` | No explicit extended thinking |

The exact standard endpoint must also match. These bindings do not extend to
model-name prefixes, another API route or a custom deployment. Other combinations
remain unknown. Source restrictions can narrow a qualification.

The sources are the [GPT-5](https://developers.openai.com/api/docs/models/gpt-5) and
[GPT-4.1](https://developers.openai.com/api/docs/models/gpt-4.1) model contracts,
the [OpenAI function-calling guide](https://developers.openai.com/api/docs/guides/function-calling),
and the Claude [tool-use](https://platform.claude.com/docs/en/agents-and-tools/tool-use/overview)
and [streaming](https://platform.claude.com/docs/en/build-with-claude/streaming) contracts.
Review is pinned in the adapter evidence. It is separate from live account access.

GPT-6 Astra and GPT-6.1 Sol tool calls require Responses. The Chat Completions
binding reports tools as unsupported for those exact models. Kimi K3.1 Pro and its
high-speed variant require full reasoning continuation. Their exact native model
and reasoning profiles remain unqualified. Native selection refuses those routes
before credential access. Their [publisher contract](https://platform.kimi.ai/docs/guide/kimi-k3-1-pro-quickstart)
requires returning the complete assistant message. This is an adapter restriction,
not a statement that those models lack tools or streaming.

A custom provider can declare a compatible profile for an exact discovered model:

```json
{"swarmie_capabilities":{"profile":"swarmie.text-tools.v1","tools":true,"streaming":true}}
```

The declaration stays bound to the discovered endpoint and model. It combines with
the tested concrete protocol adapter. Booleans without the profile do not qualify
support. A false declaration narrows support. It cannot override a known
incompatible adapter/model binding. Explicit reasoning needs a separately reviewed
profile. No declaration grants native compaction or execution authority.
