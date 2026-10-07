use mpai::{Error, Result};
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

struct Bridge {
    client: reqwest::Client,
    url: String,
    extension: String,
    token: Option<String>,
}
impl Bridge {
    async fn invoke(&self, command: &str, arguments: Value) -> Result<Value> {
        let mut request = self.client.post(&self.url).json(&json!({
            "jsonrpc":"2.0", "id":1, "method":"extension/invoke",
            "params":{"extensionId":self.extension,"command":command,"arguments":arguments}
        }));
        if let Some(token) = &self.token {
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
                json!({"protocolVersion":"2024-11-05","capabilities":{"tools":{}},"serverInfo":{"name":"mpai-extension-tools","version":env!("CARGO_PKG_VERSION")}}),
            ),
            "ping" => Ok(json!({})),
            "tools/list" => {
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
}

pub async fn serve(url: String, extension: String, token: Option<String>) -> Result<()> {
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
    };
    let mut input = BufReader::new(tokio::io::stdin());
    let mut output = tokio::io::stdout();
    loop {
        use tokio::io::AsyncReadExt;
        let mut line = String::new();
        let size = (&mut input)
            .take(1024 * 1024 + 1)
            .read_line(&mut line)
            .await?;
        if size == 0 {
            return Ok(());
        }
        if size > 1024 * 1024 {
            return Err(Error::Invalid("MCP request exceeds 1 MiB".into()));
        }
        let response = match serde_json::from_str::<Value>(&line) {
            Ok(request) if request.get("id").is_none() => continue,
            Ok(request) => {
                let id = request["id"].clone();
                if request["jsonrpc"] != "2.0"
                    || !request["method"].is_string()
                    || !(id.is_null() || id.is_string() || id.is_number())
                {
                    json!({"jsonrpc":"2.0","id":null,"error":{"code":-32600,"message":"invalid request"}})
                } else {
                    match bridge.handle(&request).await {
                        Ok(result) => json!({"jsonrpc":"2.0","id":id,"result":result}),
                        Err(error) => {
                            json!({"jsonrpc":"2.0","id":id,"error":{"code":if matches!(error,Error::Unsupported(_)) {-32601} else {-32602},"message":error.to_string()}})
                        }
                    }
                }
            }
            Err(_) => {
                json!({"jsonrpc":"2.0","id":null,"error":{"code":-32700,"message":"parse error"}})
            }
        };
        output.write_all(&serde_json::to_vec(&response)?).await?;
        output.write_all(b"\n").await?;
        output.flush().await?;
    }
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
}
