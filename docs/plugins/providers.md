# Providers

`swarmie-providers` defines model requests, results and progress. It has no Agent Loop,
scheduler or storage dependency. `swarmie-model-provider-ai` implements this API
for HTTP providers and can expose it as a Tembra orb.

The shared Rust API lives in `plugins/providers/`, alongside its existing browser package.
Agent Loop and provider implementations share these types directly. This dependency does
not install an Orb, select a concrete provider or require PluginManager.

## Make a request

The caller supplies a live Tembra context. Run this function inside the application's
Tokio I/O runtime. Set `MODEL_API_KEY` through the application's credential setup.

```rust
use std::rc::Rc;
use swarmie_model_provider_ai::{
    Connection, ConnectionId, EnvironmentCredentials, AiModel, ModelId, Provider,
    ModelProtection,
};
use swarmie_providers::{
    AgentContext, Message, Model, ModelRequest, ModelResponse, OperationId,
};

async fn generate(context: &tembra::Context) -> tembra::Result<ModelResponse> {
    let model_id = ModelId::new("chosen-model")?;
    let mut connection = Connection::new(
        ConnectionId::new("primary")?,
        Provider::OpenAi,
        [model_id.clone()],
    );
    connection.credential_key = Some("MODEL_API_KEY".into());
    let model = AiModel::new(
        connection, model_id, Rc::new(EnvironmentCredentials), ModelProtection::Unprotected,
    )?;
    let request = ModelRequest {
        context: AgentContext {
            sources: None,
            instructions: "Be concise.".into(),
            messages: vec![Message::User { content: "Explain a lease.".into() }],
        },
        tools: vec![],
    };
    model.complete(context, OperationId::new(), &request).await
}
```

`AgentContext` holds host instructions, optional captured sources and canonical messages.
Providers encode and measure `AgentContext::model_messages()` to include current sources.
See [Skills and project instructions](project-context.md). Tembra's `Context` carries
call authority and cancellation. These types have separate roles.

## Compose an orb

Pass `Configuration { connections }`, `Rc<dyn Credentials>` and `ModelProtection` to
`swarmie_model_provider_ai::orb`, then install the returned orb in the host.
Resolve `SERVICE` with `ModelKey::segments()`: the key contains the connection ID
and model ID. Each connection admits an explicit list of models.

A consuming orb can use `ModelService::bind(&declaration, host, SERVICE, key.segments())`.
Its managed reference uses the declaring object's principal and releases when its
resource closes. No Plugin Manager is required.

`Provider` selects a configured route. A model name cannot select a different
provider. `Credentials::acquire` receives the admitted physical attempt and returns
the credential for that attempt. Final content admission occurs before credential
acquisition. Configuration stores the lookup key, not the secret value.
Custom endpoints accept HTTP or HTTPS without embedded
credentials, query parameters or fragments.

## Attach model metadata

Use `Connection::model_identity` to obtain the exact catalog route. It includes the
provider, versioned adapter, endpoint, connection deployment and model. An absent
endpoint uses the configured adapter's actual default. It does not infer a provider
from a model name.

Use `Connection::configuration_revision` with the selected model and explicit
reasoning keyword. This digest includes the connection configuration and canonical
reasoning value. It does not read credential values. Resolve a catalog Snapshot for
that identity and revision, then retain the returned Arc.

Apply reasoning before `AiModel::with_metadata`. Attachment checks the exact
identity, revision and required context/input/output limits. It retains the same
immutable Arc without source refresh or network I/O. A later reasoning change that
invalidates the revision is refused. This message adapter refuses metadata that
claims native compaction support. Measurement and budget decisions are separate
from metadata attachment.

See [Model catalog](model-catalog.md) for source resolution and cache ownership.

## Capture model construction

`ModelFactory::capture` receives an `AccountRecord` and a resolved `ModelSelection`.
The account owner resolves defaults and auxiliary model selection before capture.
The concrete adapter validates reasoning and retains the exact configuration.

`CapturedModel` holds metadata inputs and a single-use `ModelConstruction`.
Its constructor checks the identity, configuration revision and source identities.
The metadata inputs use the existing Catalog types. They do not load sources or
read credentials. `CredentialRequirement::None` means an explicitly configured
credential-free route. `CredentialRequirement::Account` requires the authorized account's
credential; it is not a fallback when custody fails.

Resolve metadata with the existing Catalog, then call `CapturedModel::construct`.
This consumes the capture. It checks the Snapshot preparation and credential
presence before it calls the adapter. The adapter must also check the Snapshot
against its retained configuration. The result implements the neutral `Model`
interface. Construction does not dispatch a request or grant invocation authority.
The model keeps its physical admission and credential checks.

`AiModelFactory::new(ModelProtection)` binds the concrete AI factory to an
explicit policy. Capture validates the saved adapter fields, selected model and
reasoning. It returns exact source bindings and qualification records. The caller
loads those sources through Catalog. The factory has no source loader or cache.
Construction attaches the same Snapshot before the model can prepare a request.
The ChatGPT-plan authorization profile is unavailable in this factory until its
metadata and request profile are qualified. A discovery declaration does not enable it.

`AccountRecord` retains the public account and flat private configuration fields
for native persistence. Its constructor rejects a private `account` key. The
adapter must exclude secrets. Publish only the public account through RPC or State.
The record has no Debug implementation.

## Supply account protocols

`AiAccountDriver::new(registration_scope)` implements the neutral account driver.
Supply the existing persisted registration UUID. The constructor performs no I/O.
Inject this driver and the native stores into `swarmie_providers::accounts::Accounts`.
Accounts controls principal checks, defaults, association, custody and publication.
The driver performs discovery and the existing provider-auth protocols. It has no
account store, credential store or renewal cache.

API keys remain separate from encoded authorization credentials. Local accounts
explicitly require no custody. ChatGPT credentials must match the registration
scope and the captured account identity before reuse or refresh. The driver returns
a candidate or replacement; it does not save either value. Accounts assigns account
IDs and retains pending results until publication is recognized.

The concrete driver also exposes two pure inherent methods. `protocol_binding`
returns a fixed 32-byte digest of its registration and exact nonsecret protocol
options. It performs no discovery or credential access. Owner and account checks
remain separate; the digest grants no authority.

`needs_renewal` validates the captured account and credential without I/O. API
keys return false. Valid authorization credentials return true when expiry is at
most 60 seconds after the supplied time, with saturating arithmetic. `renew` uses
the same decision helper. Its owner must retain the same account, credential
generation/value and time, and confirm renewal intent before remote exchange.
These methods do not activate a recovery workflow or change the live account
store and driver interfaces.

The authorization flow delegates callback validation, polling, cancellation and
physical close to provider-auth. Its public instructions retain the existing browser
contract. Native fixture options are trusted configuration, not RPC inputs. A local
catalog override preserves its actual source endpoint and cannot establish production
metadata. Sign-in and model discovery do not qualify ChatGPT-plan inference.

Local discovery also does not establish model limits. Without qualified provider
or deployment facts, required context and output limits remain unknown and model
selection is unavailable. The driver does not infer them from model names or use
fallback limits. Synthetic limit fields in fixtures qualify the composition seam,
not the limits of an ordinary local deployment.

See [Provider accounts](provider-accounts.md) for publication outcomes, reconciliation
and the separate persistent recovery requirements.

## Prepare before admission

The optional `Model::prepare` capability returns a single-use native
`PreparedModelRequest`. Its safe `PreparedDescription` identifies the operation,
exact route, metadata generation, output reserve, mode, body fingerprint and
measurement. The handle retains the immutable final provider body privately. It
cannot be cloned or serialized. Dropping it does not dispatch a request.

The operational model includes `metadata_freshness` from the same Catalog
snapshot. `prepared_at_ms` is the snapshot preparation time, not a live
verification time. `valid_until_ms` is its reuse boundary. A null boundary does
not mean that every fact stays fresh. `stale_limit_evidence` counts the limit
evidence admitted within the configured stale margin. These facts are separate
from an inspection that describes a previous request.

Both timestamp fields use canonical decimal strings on the wire. They preserve
the complete `u64` range. Clients must reject numeric JSON timestamps and must
not round a timestamp through a floating-point number. The Context summary
formats calendar values only after checking their range. Snapshot timing does
not grant model authority or replace the Catalog's admission checks.

`output_reserve` is the single numeric output reservation. The typed
`output_reservation` states its basis. `sent_cap` requires that exact cap in the
retained final payload. The AI adapter checks the selected protocol field before it
creates the prepared handle. Missing, ambiguous or different caps are refused.

`native_maximum` requires the operational model's separate qualified
`native_output_maximum` capability and an equal reservation. The effective
output ceiling can include lower policy restrictions. It does not prove a
provider maximum. A native maximum above that ceiling is refused; the consumer
must not reduce the claimed maximum to fit the policy. Fixed request comparisons
include the reservation basis. Recovery compares the complete safe description.
The current AI adapter adapter emits sent caps. The capability field does not qualify
an uncapped profile by itself.

The AI adapter finalizes all provider additions before measuring, protection admission and
credential resolution. Execution sends the retained bytes without rebuilding the
request. The output reserve is also the actual maximum-output option sent to the
provider. Required tools and streaming capabilities must be qualified; Unknown
metadata fails locally. The ordinary RPC `ModelService` does not transport this
native handle and explicitly lacks the optional capability.

The current AI adapter token measurement is **estimated**: final UTF-8 JSON bytes divided
by four, rounded up. Attribution partitions those bytes into instructions,
conversation records, tools and remaining protocol fields, then distributes the
estimate by byte share. It is not a tokenizer, reported usage or proof that a model
will accept the request. Byte counts and fingerprint equality are exact. MCP tool
definitions are part of tools and are not counted twice. Safe descriptions contain
no prompt, tool payload, raw body or credential values.

Agent Loop opts into this capability with `Agent::with_preparation`. It runs
preparation and pure policy before confirming model intent, then executes that same
handle only after confirmation. A local preflight refusal leaves the turn ready to
prepare and retains its input. Confirmed pending work instead uses
`recover_prepared` with its saved safe description. Unknown outcomes never prepare
or dispatch again. SafeToRetry must reproduce the entire description before a new
dispatch; a changed model, body or metadata generation fails recovery.

## Observe progress

Call `Model::stream` with a `Progress` receiver. Each `ModelProgress` contains an
operation ID and a cumulative text preview. Replace the displayed preview;
do not append each update as a new text fragment.

`Progress::bind` declares the object's Tembra State as `Option<ModelProgress>`.
An object can declare only one State. If it already has State, use `Progress::new`
and publish through that composition instead. The receiver must return promptly.

Subscribe before invoking the streaming method. State retains one latest value per
object. Filter updates by operation ID when operations overlap. `ModelService`
subscribes before dispatch and skips the retained value from an earlier operation.

Previews are transient. They contain no tool arguments and cannot authorize tool
execution, prove completion or serve as checkpoints. Only the returned
`ModelResponse` contains the final message, tool calls and usage. The default
`Model::stream` supports providers without native streaming by calling `complete`.

The AI provider sends the first text preview immediately. It merges later
updates within `progress_interval_ms` and flushes pending text when the deadline
expires or the final response arrives. The default interval is 100 ms.

## Limits and recovery

The neutral format exposes text and JSON tool calls. Assistant messages retain
an opaque provider record from `swarmie-ai`. This record preserves native reasoning,
signatures, continuation fields and reported usage. Preparation verifies that the
visible text and calls still match it. An invalid or changed record refuses the
next request. Credential reflection removes the native continuation and leaves a
safe unavailable marker; it never replays altered signed state. Provider tool aliases contain 1–64 ASCII letters, digits, underscores or hyphens.
The adapter maps canonical tool names to these aliases and restores them in results. Parameter schemas must have object type. Historical tool results must
match the preceding calls in order, with no pending calls at request dispatch.

The default limits are 16 MiB for the encoded request, 512 KiB for the physical
response and visible decoded response, and 120 seconds for HTTP work. Native
continuation records are retained separately from the visible response budget.
These limits do not bound every allocation. Complete requests use a total deadline.
Streams use separate header and idle-read deadlines.

The provider does not retry HTTP requests or follow redirects. Cancellation
suppresses progress and can prevent dispatch. After dispatch, the host retains the AI terminal result until the physical HTTP
future settles. Managed Tembra calls retain their invocation ownership. Cancellation cannot
prove that remote computation or billing stopped. Direct callers own their futures.

`recover` returns `Recovery::Completed`, `SafeToRetry` or `Unknown`. The AI
provider returns `Unknown`: it has no portable remote-operation lookup. A local
`OperationId` does not make an HTTP request idempotent. An interrupted or incomplete
stream therefore does not permit automatic replay.

Enable the `mock` feature on `swarmie-providers` or `swarmie-model-provider-ai` for
scripted test providers. The feature is off by default. It does not change the
production model API.
