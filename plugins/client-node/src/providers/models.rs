fn whitespace(character: char) -> bool {
    character.is_whitespace() || character == '\u{feff}'
}

pub(super) fn public_label(label: &str, fallback: &str) -> String {
    let label = label.trim_matches(whitespace);
    let label = if label.is_empty() || label.chars().any(char::is_control) {
        fallback
    } else {
        label
    };
    let mut end = label.len().min(512);
    while !label.is_char_boundary(end) {
        end -= 1;
    }
    label[..end].to_owned()
}

use super::metadata::DiscoveredLimits;
use super::qualifications::DeclaredCapabilities;
use super::{catalog::ProviderSpec, failure};
use serde_json::Value;
use sha2::{Digest, Sha256};
use swarmie_ai::catalog::{Evidence, Layer, LimitClaim, Limits, Precision};
use swarmie_providers::settings::ModelInfo;

pub(super) struct Discovery {
    pub models: Vec<ModelInfo>,
    pub limits: DiscoveredLimits,
}

fn declared_capabilities(
    entry: &Value,
    endpoint: &str,
    model: &str,
    now: u64,
) -> tembra::Result<Option<DeclaredCapabilities>> {
    let Some(value) = entry.get("swarmie_capabilities") else {
        return Ok(None);
    };
    let fields = value
        .as_object()
        .ok_or_else(|| failure("the provider returned invalid model capabilities"))?;
    let boolean = |name: &str| {
        fields
            .get(name)
            .filter(|value| !value.is_null())
            .map(|value| {
                value
                    .as_bool()
                    .ok_or_else(|| failure("the provider returned invalid model capabilities"))
            })
            .transpose()
    };
    let tools = boolean("tools")?;
    let streaming = boolean("streaming")?;
    let profile = fields
        .get("profile")
        .filter(|value| !value.is_null())
        .map(|value| {
            value
                .as_str()
                .filter(|value| value.len() <= 256 && !value.chars().any(char::is_control))
                .map(str::to_owned)
                .ok_or_else(|| failure("the provider returned an invalid model profile"))
        })
        .transpose()?;
    let encoded = serde_json::to_vec(&(tools, streaming, &profile))
        .map_err(|_| failure("model capabilities could not be encoded"))?;
    Ok(Some(DeclaredCapabilities {
        tools,
        streaming,
        profile,
        evidence: Evidence {
            source: format!("{endpoint}#model={model}"),
            revision: format!("{:x}", Sha256::digest(encoded)),
            layer: Layer::Provider,
            observed_at_ms: now,
            expires_at_ms: None,
        },
    }))
}

fn declared_limits(
    entry: &Value,
    endpoint: &str,
    model: &str,
    now: u64,
) -> tembra::Result<Option<Limits>> {
    let Some(value) = entry.get("limit") else {
        return Ok(None);
    };
    let fields = value
        .as_object()
        .ok_or_else(|| failure("the provider returned invalid model limits"))?;
    let mut limits = Limits::default();
    let revision = format!(
        "{:x}",
        Sha256::digest(
            serde_json::to_vec(value)
                .map_err(|_| failure("the model metadata could not be encoded"))?
        )
    );
    let evidence = Evidence {
        source: format!("{endpoint}#model={model}"),
        revision,
        layer: Layer::Provider,
        observed_at_ms: now,
        expires_at_ms: None,
    };
    for (name, claims) in [
        ("context", &mut limits.context),
        ("input", &mut limits.input),
        ("output", &mut limits.output),
    ] {
        if let Some(value) = fields.get(name).filter(|value| !value.is_null()) {
            let tokens = value
                .as_u64()
                .ok_or_else(|| failure("the provider returned invalid model limits"))?;
            if let Some(tokens) = std::num::NonZeroU64::new(tokens) {
                claims.push(LimitClaim {
                    tokens,
                    precision: Precision::Reported,
                    evidence: evidence.clone(),
                });
            }
        }
    }
    Ok(Some(limits))
}

fn client() -> tembra::Result<reqwest::Client> {
    reqwest::Client::builder()
        .user_agent(concat!("swarmie/", env!("CARGO_PKG_VERSION")))
        .timeout(std::time::Duration::from_secs(20))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|_| failure("the provider client could not start"))
}

async fn read_catalog(mut response: reqwest::Response, invalid: &str) -> tembra::Result<Value> {
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| failure("the model catalog could not be read"))?
    {
        if bytes.len() + chunk.len() > 2 * 1024 * 1024 {
            return Err(failure("the model catalog exceeds the response limit"));
        }
        bytes.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&bytes).map_err(|_| failure(invalid))
}

fn valid_id(id: &str) -> bool {
    !id.trim_matches(whitespace).is_empty() && id.len() <= 255 && !id.chars().any(char::is_control)
}

pub(super) async fn list(
    spec: &ProviderSpec,
    endpoint: &str,
    credential: Option<&str>,
) -> tembra::Result<Discovery> {
    let client = client()?;
    let suffix = if spec.id == "ollama" {
        "api/tags"
    } else {
        "models"
    };
    let mut request = client.get(format!("{}/{suffix}", endpoint.trim_end_matches('/')));
    if let Some(credential) = credential {
        request = match spec.id {
            "anthropic" => request
                .header("x-api-key", credential)
                .header("anthropic-version", "2023-06-01"),
            _ => request.bearer_auth(credential),
        };
    }
    let response = request
        .send()
        .await
        .map_err(|_| failure("the provider could not be reached"))?;
    if !response.status().is_success() {
        return Err(failure(
            if response.status().as_u16() == 401 || response.status().as_u16() == 403 {
                "the provider refused this API key"
            } else {
                "the provider could not list its models"
            },
        ));
    }
    let json = read_catalog(response, "the provider returned an invalid model catalog").await?;
    let entries = json
        .get(if spec.id == "ollama" {
            "models"
        } else {
            "data"
        })
        .and_then(Value::as_array)
        .ok_or_else(|| failure("the provider returned an invalid model catalog"))?;
    let mut models = Vec::new();
    let mut limits = DiscoveredLimits {
        endpoint: endpoint.into(),
        ..Default::default()
    };
    let now = swarmie_providers::auth::now_ms();
    for entry in entries {
        let field = if spec.id == "ollama" { "name" } else { "id" };
        let Some(id) = entry.get(field).and_then(Value::as_str) else {
            continue;
        };
        if !valid_id(id) {
            continue;
        }
        if let Some(value) = declared_limits(entry, endpoint, id, now)? {
            if limits.models.get(id).is_some_and(|prior| prior != &value) {
                return Err(failure("the provider returned conflicting model limits"));
            }
            limits.models.insert(id.into(), value);
        }
        if let Some(value) = declared_capabilities(entry, endpoint, id, now)? {
            if limits
                .capabilities
                .get(id)
                .is_some_and(|prior| prior != &value)
            {
                return Err(failure(
                    "the provider returned conflicting model capabilities",
                ));
            }
            limits.capabilities.insert(id.into(), value);
        }
        let label = entry
            .get("display_name")
            .or_else(|| entry.get("displayName"))
            .and_then(Value::as_str)
            .unwrap_or(id);
        models.push(ModelInfo {
            id: id.into(),
            label: public_label(label, id),
            efforts: vec![],
        });
    }
    models.sort_by(|a, b| a.id.cmp(&b.id));
    models.dedup_by(|a, b| a.id == b.id);
    if models.is_empty() {
        return Err(failure("this provider has no available models"));
    }
    Ok(Discovery { models, limits })
}

pub(super) async fn oauth_models(
    credential: &swarmie_providers::auth::Credential,
    endpoint: &str,
) -> tembra::Result<Discovery> {
    let client = client()?;
    let response = client
        .get(format!("{}/models", endpoint.trim_end_matches('/')))
        .bearer_auth(credential.access_token.as_str())
        .header("Accept", "application/json")
        .send()
        .await
        .map_err(|_| failure("the authorized model catalog could not be reached"))?;
    if !response.status().is_success() {
        return Err(failure(&format!(
            "OpenAI refused the model catalog request (HTTP {}).",
            response.status().as_u16()
        )));
    }
    let json = read_catalog(response, "the authorized model catalog is invalid").await?;
    let entries = json
        .get("models")
        .and_then(Value::as_array)
        .ok_or_else(|| failure("the authorized model catalog is invalid"))?;
    let mut models = Vec::new();
    let mut limits = DiscoveredLimits {
        endpoint: endpoint.into(),
        ..Default::default()
    };
    let now = swarmie_providers::auth::now_ms();
    for entry in entries {
        if entry.get("visibility").and_then(Value::as_str) != Some("list") {
            continue;
        }
        let Some(id) = entry.get("slug").and_then(Value::as_str) else {
            continue;
        };
        if !valid_id(id) {
            continue;
        }
        if let Some(value) = declared_limits(entry, endpoint, id, now)? {
            if limits.models.get(id).is_some_and(|prior| prior != &value) {
                return Err(failure("the provider returned conflicting model limits"));
            }
            limits.models.insert(id.into(), value);
        }
        if let Some(value) = declared_capabilities(entry, endpoint, id, now)? {
            if limits
                .capabilities
                .get(id)
                .is_some_and(|prior| prior != &value)
            {
                return Err(failure(
                    "the provider returned conflicting model capabilities",
                ));
            }
            limits.capabilities.insert(id.into(), value);
        }
        let label = entry
            .get("display_name")
            .and_then(Value::as_str)
            .unwrap_or(id);
        let efforts = entry
            .get("supported_reasoning_levels")
            .and_then(Value::as_array)
            .map(|levels| {
                [
                    "none", "zero", "minimal", "low", "medium", "high", "xhigh", "max",
                ]
                .into_iter()
                .filter(|effort| {
                    levels.iter().any(|level| {
                        level
                            .get("effort")
                            .and_then(Value::as_str)
                            .is_some_and(|value| value.eq_ignore_ascii_case(effort))
                    })
                })
                .map(str::to_owned)
                .collect()
            })
            .unwrap_or_default();
        models.push(ModelInfo {
            id: id.into(),
            label: public_label(label, id),
            efforts,
        });
    }
    models.sort_by(|a, b| a.id.cmp(&b.id));
    models.dedup_by(|a, b| a.id == b.id);
    if models.is_empty() {
        return Err(failure("this authorized account has no available models"));
    }
    Ok(Discovery { models, limits })
}

#[cfg(test)]
mod tests {
    use super::*;
    use swarmie_providers::auth::{Credential, Provider};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    #[test]
    fn model_capabilities_require_an_explicit_namespace_and_typed_declaration() {
        let endpoint = "https://provider.example/v1";
        assert!(
            declared_capabilities(
                &serde_json::json!({"tool_call":true,"capabilities":{"tools":true}}),
                endpoint,
                "model",
                10
            )
            .unwrap()
            .is_none()
        );
        let row = serde_json::json!({"swarmie_capabilities":{"tools":true,"streaming":false,"profile":"swarmie.text-tools.v1"},"base_instructions":"private unrelated data"});
        let declared = declared_capabilities(&row, endpoint, "model", 10)
            .unwrap()
            .unwrap();
        assert_eq!(declared.tools, Some(true));
        assert_eq!(declared.streaming, Some(false));
        assert!(
            !serde_json::to_string(&declared)
                .unwrap()
                .contains("private")
        );
        for value in [serde_json::json!(1), serde_json::json!("yes")] {
            assert!(
                declared_capabilities(
                    &serde_json::json!({"swarmie_capabilities":{"tools":value}}),
                    endpoint,
                    "model",
                    10
                )
                .is_err()
            );
        }
    }

    #[test]
    fn provider_limits_are_optional_explicit_and_do_not_include_other_model_payload() {
        let endpoint = "https://provider.example/v1";
        assert!(
            declared_limits(
                &serde_json::json!({"context_window":1000}),
                endpoint,
                "model",
                10
            )
            .unwrap()
            .is_none()
        );
        let record = declared_limits(&serde_json::json!({"limit":{"context":1000,"input":0,"output":100},"base_instructions":"sensitive unrelated payload"}), endpoint, "model", 10).unwrap().unwrap();
        assert_eq!(record.context[0].tokens.get(), 1000);
        assert!(record.input.is_empty());
        assert!(
            !serde_json::to_string(&record)
                .unwrap()
                .contains("sensitive")
        );
        for value in [
            serde_json::json!(-1),
            serde_json::json!(true),
            serde_json::json!(1.5),
        ] {
            assert!(
                declared_limits(
                    &serde_json::json!({"limit":{"context":value}}),
                    endpoint,
                    "model",
                    10
                )
                .is_err()
            );
        }
    }

    #[tokio::test]
    async fn kimi_catalog_uses_bearer_auth_and_openai_compatible_model_discovery() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let worker = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut buffer = [0; 4096];
            let size = stream.read(&mut buffer).await.unwrap();
            let request = String::from_utf8_lossy(&buffer[..size]);
            assert!(request.starts_with("GET /models HTTP/1.1"));
            assert!(
                request
                    .to_ascii_lowercase()
                    .contains("authorization: bearer synthetic-key")
            );
            let body = r#"{"data":[{"id":"kimi-fixture","display_name":"Kimi fixture"}]}"#;
            let response = format!(
                "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                body.len()
            );
            stream.write_all(response.as_bytes()).await.unwrap();
        });
        let spec = super::super::catalog::specifications()
            .into_iter()
            .find(|spec| spec.id == "kimi")
            .unwrap();
        let models = list(&spec, &endpoint, Some("synthetic-key"))
            .await
            .unwrap()
            .models;
        worker.await.unwrap();
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].id, "kimi-fixture");
        assert!(models[0].efforts.is_empty());
    }

    #[tokio::test]
    async fn oauth_catalog_retains_the_previous_transport_budget_and_projects_only_model_fields() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let body = serde_json::json!({"models":[{"slug":"fixture-model","visibility":"list","display_name":"Fixture model","supported_reasoning_levels":[{"effort":"medium"}],"base_instructions":"x".repeat(600 * 1024)}]}).to_string();
        let worker = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut buffer = [0; 4096];
            assert!(stream.read(&mut buffer).await.unwrap() > 0);
            let header = format!(
                "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
                body.len()
            );
            stream.write_all(header.as_bytes()).await.unwrap();
            let _ = stream.write_all(body.as_bytes()).await;
        });
        let credential = Credential {
            provider: Provider::Chatgpt,
            access_token: "fixture-access".to_owned().into(),
            refresh_token: "fixture-refresh".to_owned().into(),
            expires_at_ms: u64::MAX,
            chatgpt: None,
            endpoint,
        };
        let models = oauth_models(&credential, &credential.endpoint).await;
        worker.await.unwrap();
        let models = models.unwrap().models;
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].id, "fixture-model");
        assert_eq!(models[0].efforts, ["medium"]);
        assert!(serde_json::to_vec(&models).unwrap().len() < 256);
    }
    #[tokio::test]
    async fn both_catalogs_reject_whitespace_and_feff_model_ids_before_using_label_fallbacks() {
        for oauth in [false, true] {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let endpoint = format!("http://{}", listener.local_addr().unwrap());
            let rows = [" ", "\u{feff}", "\u{feff} \u{00a0}", "fixture-model"]
                .into_iter()
                .map(|id| {
                    if oauth {
                        serde_json::json!({"slug":id,"visibility":"list","display_name":"\u{feff}"})
                    } else {
                        serde_json::json!({"name":id,"display_name":"\u{feff}"})
                    }
                })
                .collect::<Vec<_>>();
            let body = serde_json::json!({"models":rows}).to_string();
            let worker = tokio::spawn(async move {
                let (mut stream, _) = listener.accept().await.unwrap();
                let mut buffer = [0; 4096];
                assert!(stream.read(&mut buffer).await.unwrap() > 0);
                let response = format!(
                    "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                    body.len()
                );
                stream.write_all(response.as_bytes()).await.unwrap();
            });
            let models = if oauth {
                let credential = Credential {
                    provider: Provider::Chatgpt,
                    access_token: "fixture-access".to_owned().into(),
                    refresh_token: "fixture-refresh".to_owned().into(),
                    expires_at_ms: u64::MAX,
                    chatgpt: None,
                    endpoint: endpoint.clone(),
                };
                oauth_models(&credential, &endpoint).await.unwrap().models
            } else {
                let spec = super::super::catalog::specifications()
                    .into_iter()
                    .find(|spec| spec.id == "ollama")
                    .unwrap();
                list(&spec, &endpoint, None).await.unwrap().models
            };
            worker.await.unwrap();
            assert_eq!(
                models.len(),
                1,
                "only the nonblank canonical model id can enter the projection"
            );
            assert_eq!(models[0].id, "fixture-model");
            assert_eq!(models[0].label, "fixture-model");
        }
    }
}
