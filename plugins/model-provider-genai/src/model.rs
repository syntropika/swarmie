mod streaming;

use crate::{
    Connection, Credentials, ModelId, conversion,
    error::{Reason, invalid, upstream},
};
use futures::{FutureExt, future::LocalBoxFuture};
use genai::{
    Client,
    resolver::{AuthData, Endpoint, ProviderConfig},
};
use std::time::Duration;
use swarmie_providers::{Model, ModelRequest, ModelResponse, OperationId, Progress, Recovery};
use tembra::{Code, Context, Error};

/// One configured model client. The caller owns direct futures; a native orb
/// delegates physical ownership and resource cleanup to Tembra. No HTTP retry,
/// operation receipt store or independent executor is created here.
pub struct GenaiModel {
    client: Client,
    connection: Connection,
    model: ModelId,
}
impl GenaiModel {
    fn request(&self, request: &ModelRequest) -> tembra::Result<genai::chat::ChatRequest> {
        self.budget(
            serde_json::to_vec(request)
                .map_err(|_| invalid(Reason::Request, "request cannot be encoded"))?
                .len(),
            true,
        )?;
        conversion::request(request)
    }

    fn budget(&self, bytes: usize, request: bool) -> tembra::Result<()> {
        let (limit, reason) = if request {
            (self.connection.request_bytes.get(), Reason::Request)
        } else {
            (self.connection.response_bytes.get(), Reason::Response)
        };
        if bytes > limit {
            return Err(Error::new(
                Code::Exhausted,
                reason.as_str(),
                "model payload exceeds its configured decoded limit",
            ));
        }
        Ok(())
    }

    fn response(
        &self,
        response: genai::chat::ChatResponse,
        request: &ModelRequest,
    ) -> tembra::Result<ModelResponse> {
        let output = conversion::response(response, request)?;
        self.budget(
            serde_json::to_vec(&output)
                .map_err(|_| invalid(Reason::Response, "response cannot be encoded"))?
                .len(),
            false,
        )?;
        Ok(output)
    }
    /// Call within the embedding application's Tokio I/O runtime.
    pub fn new(
        connection: Connection,
        model: ModelId,
        credentials: &dyn Credentials,
    ) -> tembra::Result<Self> {
        connection.validate()?;
        if !connection.models.contains(&model) {
            return Err(invalid(
                Reason::Configuration,
                "model is not admitted by this connection",
            ));
        }
        tokio::runtime::Handle::try_current().map_err(|_| {
            Error::new(
                Code::Unavailable,
                Reason::Runtime.as_str(),
                "model HTTP requires the host's Tokio I/O runtime",
            )
        })?;
        let auth = match &connection.credential_key {
            Some(key) => AuthData::from_single(
                credentials
                    .resolve(key)
                    .filter(|v| !v.is_empty())
                    .ok_or_else(|| {
                        Error::new(
                            Code::Unavailable,
                            Reason::Credentials.as_str(),
                            "model credential is unavailable",
                        )
                    })?,
            ),
            None => AuthData::None,
        };
        let mut config = ProviderConfig::from_auth(auth);
        if let Some(url) = &connection.endpoint {
            config = config.with_endpoint(Endpoint::from_owned(format!(
                "{}/",
                url.trim_end_matches('/')
            )));
        }
        let http = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            .connect_timeout(Duration::from_millis(
                connection.timeout_ms.get().min(30_000),
            ))
            .timeout(Duration::from_millis(connection.timeout_ms.get()))
            .build()
            .map_err(|_| {
                Error::new(
                    Code::Unavailable,
                    Reason::Runtime.as_str(),
                    "model HTTP client initialization failed",
                )
            })?;
        let client = Client::builder()
            .with_reqwest(http)
            .with_adapter_kind(connection.provider.adapter())
            .append_provider_config(connection.provider.adapter(), config)
            .build()
            .map_err(|_| {
                Error::new(
                    Code::Unavailable,
                    Reason::Runtime.as_str(),
                    "model client initialization failed",
                )
            })?;
        Ok(Self {
            client,
            connection,
            model,
        })
    }
}
impl Model for GenaiModel {
    fn stream<'a>(
        &'a self,
        context: &'a Context,
        operation: OperationId,
        request: &'a ModelRequest,
        progress: &'a Progress,
    ) -> LocalBoxFuture<'a, tembra::Result<ModelResponse>> {
        streaming::stream(self, context, operation, request, progress).boxed_local()
    }

    fn complete<'a>(
        &'a self,
        context: &'a Context,
        _operation: OperationId,
        request: &'a ModelRequest,
    ) -> LocalBoxFuture<'a, tembra::Result<ModelResponse>> {
        async move {
            context.check()?;
            let input = self.request(request)?;
            context.check()?;
            // Await physical HTTP settlement even if the logical waiter cancels.
            // The configured timeout bounds transport waiting; cancellation cannot
            // prove that remote computation or billing stopped.
            let response = self
                .client
                .exec_chat(self.model.as_str(), input, None)
                .await
                .map_err(upstream)?;
            let output = self.response(response, request)?;
            context.check()?;
            Ok(output)
        }
        .boxed_local()
    }
    fn recover<'a>(
        &'a self,
        context: &'a Context,
        _operation: OperationId,
        _request: &'a ModelRequest,
    ) -> LocalBoxFuture<'a, tembra::Result<Recovery<ModelResponse>>> {
        // A local operation ID does not establish remote provider idempotency.
        // genai exposes no portable operation lookup; never infer safe replay.
        async move {
            context.check()?;
            Ok(Recovery::Unknown)
        }
        .boxed_local()
    }
}
