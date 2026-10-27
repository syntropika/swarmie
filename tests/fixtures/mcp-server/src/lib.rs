//! Synthetic MCP counter. Never used as a production service or provider.

use rmcp::{
    ErrorData, RoleServer, ServerHandler,
    model::{
        CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock, ListToolsResult,
        PaginatedRequestParams, ServerCapabilities, ServerConfig, Tool,
    },
    service::RequestContext,
};
use serde_json::json;
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

#[derive(Clone, Default)]
pub struct Counter {
    effects: Arc<AtomicU64>,
    catalog_reads: Arc<AtomicU64>,
    catalog_started: Arc<tokio::sync::Notify>,
    catalog_delay: std::time::Duration,
}

impl Counter {
    pub fn effects(&self) -> u64 {
        self.effects.load(Ordering::SeqCst)
    }

    pub fn with_catalog_delay(mut self, delay: std::time::Duration) -> Self {
        self.catalog_delay = delay;
        self
    }

    pub fn catalog_reads(&self) -> u64 {
        self.catalog_reads.load(Ordering::SeqCst)
    }

    pub async fn wait_for_catalog(&self) {
        while self.catalog_reads() == 0 {
            self.catalog_started.notified().await;
        }
    }
}

impl ServerHandler for Counter {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
    }

    async fn list_tools(
        &self,
        params: Option<PaginatedRequestParams>,
        _: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        self.catalog_reads.fetch_add(1, Ordering::SeqCst);
        self.catalog_started.notify_one();
        tokio::time::sleep(self.catalog_delay).await;
        let (name, next) = match params.and_then(|params| params.cursor).as_deref() {
            None => ("counter.add", Some("second")),
            Some("second") => ("counter.fail", Some("third")),
            Some("third") => ("counter.lose", None),
            _ => return Err(ErrorData::invalid_params("unknown cursor", None)),
        };
        let tool = Tool::new(name, "Synthetic counter operation", Arc::new(json!({"type":"object","properties":{"amount":{"type":"integer"}},"required":["amount"],"additionalProperties":false}).as_object().unwrap().clone()));
        let mut page = ListToolsResult::with_all_items(vec![tool]);
        page.next_cursor = next.map(str::to_owned);
        Ok(page)
    }

    async fn call_tool(
        &self,
        params: CallToolRequestParams,
        _: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        if params.name == "counter.fail" {
            let mut content = private_content();
            content.push(ContentBlock::text("synthetic known failure"));
            return Ok(CallToolResult::error(content).into());
        }
        if params.name != "counter.add" && params.name != "counter.lose" {
            return Err(ErrorData::invalid_params("unknown tool", None));
        }
        let amount = params
            .arguments
            .as_ref()
            .and_then(|args| args.get("amount"))
            .and_then(serde_json::Value::as_u64)
            .ok_or_else(|| ErrorData::invalid_params("amount must be an unsigned integer", None))?;
        let count = self.effects.fetch_add(1, Ordering::SeqCst) + 1;
        if params.name == "counter.lose" {
            return std::future::pending().await;
        }
        let mut result = CallToolResult::structured(
            json!({"amount":amount,"effect_count":count,"pid":std::process::id(),"business":{"_meta":"public-business-value"}}),
        );
        result.content.extend(private_content());
        result.meta =
            Some(serde_json::from_value(json!({"private_canary":"mcp-private-result"})).unwrap());
        Ok(result.into())
    }
}

fn private_content() -> Vec<ContentBlock> {
    serde_json::from_value(json!([
        {"type":"text","text":"Public text","_meta":{"private_canary":"nested-private-metadata"}},
        {"type":"image","data":"AA==","mimeType":"image/png","_meta":{"private_canary":"nested-private-metadata"}},
        {"type":"audio","data":"AA==","mimeType":"audio/wav","_meta":{"private_canary":"nested-private-metadata"}},
        {"type":"resource_link","uri":"fixture://link","name":"Public link","_meta":{"private_canary":"nested-private-metadata"}},
        {"type":"resource","resource":{"uri":"fixture://text","text":"Public resource","_meta":{"private_canary":"nested-private-metadata"}},"_meta":{"private_canary":"nested-private-metadata"}},
        {"type":"resource","resource":{"uri":"fixture://blob","blob":"AA==","_meta":{"private_canary":"nested-private-metadata"}},"_meta":{"private_canary":"nested-private-metadata"}}
    ])).unwrap()
}

pub async fn serve_http(
    counter: Counter,
) -> (
    String,
    tokio_util::sync::CancellationToken,
    tokio::task::JoinHandle<()>,
) {
    serve_http_with_headers(counter, Default::default()).await
}

pub async fn serve_http_with_headers(
    counter: Counter,
    headers: std::collections::BTreeMap<String, String>,
) -> (
    String,
    tokio_util::sync::CancellationToken,
    tokio::task::JoinHandle<()>,
) {
    use axum::{middleware::Next, response::IntoResponse};
    use rmcp::transport::streamable_http_server::{
        StreamableHttpServerConfig, StreamableHttpService, session::local::LocalSessionManager,
    };
    let cancel = tokio_util::sync::CancellationToken::new();
    let service: StreamableHttpService<Counter, LocalSessionManager> = StreamableHttpService::new(
        move || Ok(counter.clone()),
        Default::default(),
        StreamableHttpServerConfig::default().with_cancellation_token(cancel.child_token()),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let router =
        axum::Router::new()
            .nest_service("/mcp", service)
            .layer(axum::middleware::from_fn(
                move |request: axum::extract::Request, next: Next| {
                    let valid = headers.iter().all(|(key, expected)| {
                        request
                            .headers()
                            .get(key.as_str())
                            .and_then(|value| value.to_str().ok())
                            == Some(expected.as_str())
                    });
                    async move {
                        if valid {
                            next.run(request).await
                        } else {
                            axum::http::StatusCode::BAD_REQUEST.into_response()
                        }
                    }
                },
            ));
    let shutdown = cancel.clone();
    let task = tokio::spawn(async move {
        axum::serve(listener, router)
            .with_graceful_shutdown(shutdown.cancelled_owned())
            .await
            .unwrap();
    });
    (format!("http://{address}/mcp"), cancel, task)
}
