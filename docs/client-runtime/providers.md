# Providers

`swarmie-providers` defines model requests, results and progress. It has no Agent Loop,
scheduler or storage dependency. `swarmie-model-provider-genai` implements this API
for HTTP providers and can expose it as a Tembra orb.

The shared Rust API lives in `plugins/providers/`, alongside its existing browser package.
Agent Loop and provider implementations share these types directly. This dependency does
not install an Orb, select a concrete provider or require PluginManager.

## Make a request

The caller supplies a live Tembra context. Run this function inside the application's
Tokio I/O runtime. Set `MODEL_API_KEY` through the application's credential setup.

```rust
use swarmie_model_provider_genai::{
    Connection, ConnectionId, EnvironmentCredentials, GenaiModel, ModelId, Provider,
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
    let model = GenaiModel::new(connection, model_id, &EnvironmentCredentials)?;
    let request = ModelRequest {
        context: AgentContext {
            instructions: "Be concise.".into(),
            messages: vec![Message::User { content: "Explain a lease.".into() }],
        },
        tools: vec![],
    };
    model.complete(context, OperationId::new(), &request).await
}
```

`AgentContext` holds model instructions and messages. Tembra's `Context` carries
call authority and cancellation. These types have separate roles.

## Compose an orb

Pass `Configuration { connections }` and `Rc<dyn Credentials>` to
`swarmie_model_provider_genai::orb`, then install the returned orb in the host.
Resolve `SERVICE` with `ModelKey::segments()`: the key contains the connection ID
and model ID. Each connection admits an explicit list of models.

A consuming orb can use `ModelService::bind(&declaration, host, SERVICE, key.segments())`.
Its managed reference uses the declaring object's principal and releases when its
resource closes. No Plugin Manager is required.

`Provider` selects a configured route. A model name cannot select a different
provider. `Credentials::resolve`
returns a secret for a configured lookup key. Configuration stores that key, not
the secret value. Custom endpoints accept HTTP or HTTPS without embedded
credentials, query parameters or fragments.

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

The genai provider sends the first text preview immediately. It merges later
updates within `progress_interval_ms` and flushes pending text when the deadline
expires or the final response arrives. The default interval is 100 ms.

## Limits and recovery

The current format supports text and JSON tool calls. The genai provider rejects
reasoning continuations, signed tool calls and other content that this format
cannot preserve. Tool names must contain 1–64 ASCII letters, digits, underscores
or hyphens. Parameter schemas must have object type. Historical tool results must
match the preceding calls in order, with no pending calls at request dispatch.

The default limits are 256 KiB for the encoded request, 512 KiB for the normalized
response and 120 seconds for HTTP work. Streaming also checks accumulated text and
tool data. These limits do not bound every upstream HTTP buffer or allocation.

The provider does not retry HTTP requests or follow redirects. Cancellation
suppresses progress and can prevent dispatch. After dispatch, a managed Tembra
call keeps ownership until the physical HTTP future settles. Cancellation cannot
prove that remote computation or billing stopped. Direct callers own their futures.

`recover` returns `Recovery::Completed`, `SafeToRetry` or `Unknown`. The genai
provider returns `Unknown`: it has no portable remote-operation lookup. A local
`OperationId` does not make an HTTP request idempotent. An interrupted or incomplete
stream therefore does not permit automatic replay.

Enable the `mock` feature on `swarmie-providers` or `swarmie-model-provider-genai` for
scripted test providers. The feature is off by default. It does not change the
production model API.
