use serde_json::{Value, json};
use std::{collections::BTreeMap, sync::Arc};
use swarmie::{Error, Result};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio_util::sync::CancellationToken;

struct Bridge {
    client: reqwest::Client,
    url: String,
    extension: String,
    token: Option<String>,
    host_channel: Option<String>,
}
impl Bridge {
    async fn rpc(&self, method: &str, params: Value) -> Result<Value> {
        self.send(json!({"jsonrpc":"2.0","id":1,"method":method,"params":params}))
            .await
    }
    async fn invoke(&self, command: &str, arguments: Value) -> Result<Value> {
        self.rpc(
            "extension/invoke",
            json!({"extensionId":self.extension,"command":command,"arguments":arguments}),
        )
        .await
    }
    async fn send(&self, body: Value) -> Result<Value> {
        let mut request = self.client.post(&self.url).json(&body);
        if let Some(token) = self.host_channel.as_ref().or(self.token.as_ref()) {
            request = request.bearer_auth(token);
        }
        let mut response = request
            .send()
            .await
            .map_err(|e| Error::Provider(e.to_string()))?;
        if !response.status().is_success() {
            return Err(Error::Provider(format!(
                "daemon HTTP {}",
                response.status()
            )));
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|e| Error::Provider(e.to_string()))?
        {
            if bytes.len() + chunk.len() > 16 * 1024 * 1024 {
                return Err(Error::Invalid("extension response exceeds 16 MiB".into()));
            }
            bytes.extend_from_slice(&chunk);
        }
        let body: Value = serde_json::from_slice(&bytes)?;
        if let Some(error) = body.get("error") {
            return Err(Error::Provider(
                error["message"]
                    .as_str()
                    .unwrap_or("extension command failed")
                    .into(),
            ));
        }
        Ok(body["result"].clone())
    }
    async fn handle(&self, request: &Value) -> Result<Value> {
        match request["method"].as_str().unwrap_or_default() {
            "initialize" => Ok(
                json!({"protocolVersion":"2024-11-05","capabilities":{"tools":{}},"serverInfo":{"name":"swarmie-extension-tools","version":env!("CARGO_PKG_VERSION")}}),
            ),
            "ping" => Ok(json!({})),
            "tools/list" => {
                if self.host_channel.is_some() {
                    return self.rpc("tools/list", json!({})).await;
                }
                let mut catalog = self.invoke("tools.list", json!({})).await?;
                if let Some(tools) = catalog["tools"].as_array_mut() {
                    for tool in tools {
                        if let Some(map) = tool.as_object_mut() {
                            map.remove("command");
                        }
                    }
                }
                Ok(catalog)
            }
            "tools/call" => {
                if self.host_channel.is_some() {
                    return self.host_call(request, CancellationToken::new()).await;
                }
                let name = request["params"]["name"]
                    .as_str()
                    .ok_or_else(|| Error::Invalid("tool name is required".into()))?;
                let catalog = self.invoke("tools.list", json!({})).await?;
                let tool = catalog["tools"]
                    .as_array()
                    .and_then(|tools| tools.iter().find(|tool| tool["name"] == name))
                    .ok_or_else(|| {
                        Error::Invalid("tool is not advertised by this extension".into())
                    })?;
                let command = tool["command"].as_str().unwrap_or(name);
                let arguments = request["params"]
                    .get("arguments")
                    .cloned()
                    .unwrap_or_else(|| json!({}));
                if !arguments.is_object() {
                    return Err(Error::Invalid("tool arguments must be an object".into()));
                }
                match self.invoke(command, arguments).await {
                    Ok(result) => Ok(
                        json!({"content":[{"type":"text","text":serde_json::to_string(&result)?}],"isError":false}),
                    ),
                    Err(error) => Ok(
                        json!({"content":[{"type":"text","text":error.to_string()}],"isError":true}),
                    ),
                }
            }
            method => Err(Error::Unsupported(format!("MCP method {method}"))),
        }
    }
    async fn host_call(&self, request: &Value, cancellation: CancellationToken) -> Result<Value> {
        if cancellation.is_cancelled() {
            return Err(Error::Cancelled("MCP tool call".into()));
        }
        let receipt = self.rpc("tools/prepare", request["params"].clone()).await?;
        let id = receipt["id"]
            .as_str()
            .ok_or_else(|| Error::Provider("host omitted invocation identity".into()))?;
        let params = json!({"invocationId":id});
        let result = tokio::select! {
            biased;
            _ = cancellation.cancelled() => {
                self.rpc("tools/cancel",params.clone()).await?;
                Err(Error::Cancelled("MCP tool call".into()))
            }
            result = self.rpc("tools/execute",params.clone()) => {
                if result.is_err() { let _ = self.rpc("tools/cancel",params.clone()).await; }
                result
            }
        };
        match result {
            Ok(value) => Ok(
                json!({"content":[{"type":"text","text":serde_json::to_string(&value)?}],"isError":false}),
            ),
            Err(error) => {
                let status = self.rpc("tools/status", params).await.ok();
                let failure = json!({
                    "invocationId":id,
                    "origin":{"extensionId":receipt["extensionId"],"toolId":receipt["toolId"]},
                    "state":status.map(|value|value["state"].clone()),
                    "error":error.to_string(),
                });
                Ok(
                    json!({"content":[{"type":"text","text":serde_json::to_string(&failure)?}],"isError":true}),
                )
            }
        }
    }
}

pub async fn serve(
    url: String,
    extension: String,
    token: Option<String>,
    host_channel: Option<String>,
) -> Result<()> {
    let parsed = reqwest::Url::parse(&url).map_err(|e| Error::Invalid(e.to_string()))?;
    if !matches!(parsed.scheme(), "http" | "https")
        || !parsed.username().is_empty()
        || parsed.password().is_some()
    {
        return Err(Error::Invalid(
            "use an HTTP(S) daemon RPC URL without embedded credentials".into(),
        ));
    }
    let bridge = Bridge {
        client: reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(120))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|e| Error::Provider(e.to_string()))?,
        url,
        extension,
        token,
        host_channel,
    };
    serve_io(tokio::io::stdin(), tokio::io::stdout(), Arc::new(bridge)).await
}

async fn serve_io<I, O>(input: I, mut output: O, bridge: Arc<Bridge>) -> Result<()>
where
    I: tokio::io::AsyncRead + Unpin + Send + 'static,
    O: tokio::io::AsyncWrite + Unpin,
{
    let (tx, mut rx) = tokio::sync::mpsc::channel(16);
    let reader = tokio::spawn(async move {
        let mut input = BufReader::new(input);
        loop {
            use tokio::io::AsyncReadExt;
            let mut line = String::new();
            let result = (&mut input)
                .take(1024 * 1024 + 1)
                .read_line(&mut line)
                .await;
            let value = match result {
                Ok(0) => break,
                Ok(size) if size > 1024 * 1024 => {
                    Err(Error::Invalid("MCP request exceeds 1 MiB".into()))
                }
                Ok(_) => Ok(line),
                Err(error) => Err(error.into()),
            };
            let failed = value.is_err();
            if tx.send(value).await.is_err() || failed {
                break;
            }
        }
    });
    let result = dispatch(&mut rx, &mut output, bridge).await;
    reader.abort();
    let _ = reader.await;
    result
}

async fn dispatch<O: tokio::io::AsyncWrite + Unpin>(
    input: &mut tokio::sync::mpsc::Receiver<Result<String>>,
    output: &mut O,
    bridge: Arc<Bridge>,
) -> Result<()> {
    let mut tasks = tokio::task::JoinSet::new();
    let mut active: BTreeMap<String, CancellationToken> = BTreeMap::new();
    let mut eof = false;
    let shutdown_deadline = tokio::time::sleep(std::time::Duration::from_secs(5));
    tokio::pin!(shutdown_deadline);
    let result = async {
    loop {
        let response = tokio::select! {
            task = tasks.join_next(), if !tasks.is_empty() => {
                let (key,response) = task.expect("nonempty task set").map_err(|e|Error::Provider(e.to_string()))?;
                active.remove(&key);
                response
            }
            line = input.recv(), if !eof => {
                let Some(line) = line else {
                    eof = true;
                    shutdown_deadline.as_mut().reset(tokio::time::Instant::now()+std::time::Duration::from_secs(5));
                    for cancellation in active.values() { cancellation.cancel(); }
                    continue;
                };
                let line = line?;
                let request:Value = match serde_json::from_str(&line) {
                    Ok(request) => request,
                    Err(_) => { write_response(output,&json!({"jsonrpc":"2.0","id":null,"error":{"code":-32700,"message":"parse error"}})).await?; continue; }
                };
                if request.get("id").is_none() {
                    if request["method"] == "notifications/cancelled" {
                        let key = serde_json::to_string(&request["params"]["requestId"])?;
                        if let Some(cancellation) = active.get(&key) { cancellation.cancel(); }
                    }
                    continue;
                }
                let id = request["id"].clone();
                if request["jsonrpc"] != "2.0" || !request["method"].is_string() || !(id.is_string() || id.is_number()) {
                    json!({"jsonrpc":"2.0","id":null,"error":{"code":-32600,"message":"invalid request"}})
                } else {
                    let key = serde_json::to_string(&id)?;
                    if active.contains_key(&key) || active.len() >= 64 {
                        json!({"jsonrpc":"2.0","id":id,"error":{"code":-32600,"message":"duplicate request identity or too many active requests"}})
                    } else {
                        let cancellation = CancellationToken::new();
                        active.insert(key.clone(),cancellation.clone());
                        let bridge = bridge.clone();
                        tasks.spawn(async move {
                            let result = if bridge.host_channel.is_some() && request["method"]=="tools/call" {
                                bridge.host_call(&request,cancellation).await
                            } else {
                                tokio::select! {
                                    biased;
                                    _ = cancellation.cancelled() => Err(Error::Cancelled("MCP request".into())),
                                    value = bridge.handle(&request) => value,
                                }
                            };
                            let response = match result {
                                Ok(result) => json!({"jsonrpc":"2.0","id":id,"result":result}),
                                Err(error) => json!({"jsonrpc":"2.0","id":id,"error":{"code":if matches!(error,Error::Cancelled(_)) {-32800} else if matches!(error,Error::Unsupported(_)) {-32601} else {-32602},"message":error.to_string()}}),
                            };
                            (key,response)
                        });
                        continue;
                    }
                }
            }
            _ = &mut shutdown_deadline, if eof && !tasks.is_empty() => return Err(Error::Timeout("MCP shutdown".into())),
            else => break,
        };
        if !eof { write_response(output,&response).await?; }
    }
    Ok(())
    }.await;
    for cancellation in active.values() {
        cancellation.cancel();
    }
    if tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while tasks.join_next().await.is_some() {}
    })
    .await
    .is_err()
    {
        tasks.abort_all();
    }
    result
}
async fn write_response<O: tokio::io::AsyncWrite + Unpin>(
    output: &mut O,
    response: &Value,
) -> Result<()> {
    output.write_all(&serde_json::to_vec(response)?).await?;
    output.write_all(b"\n").await?;
    output.flush().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn tool_catalog_is_enforced_and_commands_are_translated() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let router = axum::Router::new().route("/rpc", axum::routing::post(|axum::Json(request): axum::Json<Value>| async move {
            let command = request["params"]["command"].as_str().unwrap();
            let result = if command == "tools.list" { json!({"tools":[{"name":"peers_spawn","command":"peers.spawn","description":"Start peer","inputSchema":{"type":"object"}}]}) } else { assert_eq!(command,"peers.spawn"); json!({"sessionId":"actual-peer"}) };
            axum::Json(json!({"jsonrpc":"2.0","id":request["id"],"result":result}))
        }));
        let server = tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        let bridge = Bridge {
            client: reqwest::Client::new(),
            url: format!("http://{address}/rpc"),
            extension: "resources".into(),
            token: None,
            host_channel: None,
        };
        let catalog = bridge
            .handle(&json!({"method":"tools/list"}))
            .await
            .unwrap();
        assert!(catalog["tools"][0].get("command").is_none());
        assert!(
            bridge
                .handle(&json!({"method":"tools/call","params":{"name":"credentials.set"}}))
                .await
                .is_err()
        );
        let result = bridge.handle(&json!({"method":"tools/call","params":{"name":"peers_spawn","arguments":{"cwd":"/project"}}})).await.unwrap();
        assert_eq!(result["isError"], false);
        assert!(
            result["content"][0]["text"]
                .as_str()
                .unwrap()
                .contains("actual-peer")
        );
        server.abort();
    }
    #[tokio::test]
    async fn cancellation_notification_and_eof_reach_the_host_while_other_requests_progress() {
        for eof in [false, true] {
            let entered = Arc::new(tokio::sync::Notify::new());
            let cancelled = Arc::new(tokio::sync::Notify::new());
            let observed = Arc::new(std::sync::atomic::AtomicBool::new(false));
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let address = listener.local_addr().unwrap();
            let e = entered.clone();
            let c = cancelled.clone();
            let flag = observed.clone();
            let router=axum::Router::new().route("/tools/rpc",axum::routing::post(move |headers:axum::http::HeaderMap,axum::Json(request):axum::Json<Value>| {
                let e=e.clone();let c=c.clone();let flag=flag.clone();
                async move {
                    assert_eq!(headers["authorization"],"Bearer private-tool-channel");
                    let result=match request["method"].as_str().unwrap() {
                        "tools/prepare" => json!({"id":"host-call","extensionId":"fixture","toolId":"read"}),
                        "tools/status" => json!({"state":"cancelled"}),
                        "tools/execute" => {
                            e.notify_one();c.notified().await;
                            return axum::Json(json!({"jsonrpc":"2.0","id":1,"error":{"code":-32800,"message":"cancelled"}}));
                        }
                        "tools/cancel" => { assert_eq!(request["params"]["invocationId"],"host-call");flag.store(true,std::sync::atomic::Ordering::SeqCst);c.notify_one();json!({}) },
                        method => panic!("unexpected method {method}"),
                    };
                    axum::Json(json!({"jsonrpc":"2.0","id":1,"result":result}))
                }
            }));
            let server = tokio::spawn(async move {
                axum::serve(listener, router).await.unwrap();
            });
            let bridge = Arc::new(Bridge {
                client: reqwest::Client::new(),
                url: format!("http://{address}/tools/rpc"),
                extension: "unused".into(),
                token: Some("must-not-be-used".into()),
                host_channel: Some("private-tool-channel".into()),
            });
            let (mut writer, input) = tokio::io::duplex(8192);
            let (output, reader) = tokio::io::duplex(8192);
            let task = tokio::spawn(serve_io(input, output, bridge));
            let mut reader = BufReader::new(reader).lines();
            writer.write_all(b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"tools/call\",\"params\":{\"name\":\"test\",\"arguments\":{}}}\n").await.unwrap();
            tokio::time::timeout(std::time::Duration::from_secs(2), entered.notified())
                .await
                .unwrap();
            writer
                .write_all(b"{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"ping\"}\n")
                .await
                .unwrap();
            let ping = tokio::time::timeout(std::time::Duration::from_secs(2), reader.next_line())
                .await
                .unwrap()
                .unwrap()
                .unwrap();
            assert_eq!(serde_json::from_str::<Value>(&ping).unwrap()["id"], 2);
            if eof {
                writer.shutdown().await.unwrap();
            } else {
                writer.write_all(b"{\"jsonrpc\":\"2.0\",\"method\":\"notifications/cancelled\",\"params\":{\"requestId\":1}}\n").await.unwrap();
                let cancelled =
                    tokio::time::timeout(std::time::Duration::from_secs(2), reader.next_line())
                        .await
                        .unwrap()
                        .unwrap()
                        .unwrap();
                let response: Value = serde_json::from_str(&cancelled).unwrap();
                assert_eq!(response["id"], 1);
                assert_eq!(response["result"]["isError"], true);
                let detail: Value = serde_json::from_str(
                    response["result"]["content"][0]["text"].as_str().unwrap(),
                )
                .unwrap();
                assert_eq!(
                    detail["origin"],
                    json!({"extensionId":"fixture","toolId":"read"})
                );
                assert_eq!(detail["state"], "cancelled");
                writer.shutdown().await.unwrap();
            }
            tokio::time::timeout(std::time::Duration::from_secs(2), task)
                .await
                .unwrap()
                .unwrap()
                .unwrap();
            assert!(observed.load(std::sync::atomic::Ordering::SeqCst));
            server.abort();
        }
    }
}
