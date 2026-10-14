use serde_json::{Value, json};
use std::{collections::BTreeMap, path::Path};
use swarmie::{AgentToolExecutionOverlay, Error, Result};

fn existing() -> Value {
    json!({"id":"existing","label":"Use existing sign-in","kind":"existing","help":"Reuse the provider's existing credentials on this runtime node."})
}
fn key() -> Value {
    json!({"id":"api_key","label":"API key","kind":"api_key","fields":[{"id":"apiKey","label":"API key","secret":true,"placeholder":"Paste your provider API key"}]})
}
fn login(id: &str, label: &str, kind: &str) -> Value {
    json!({"id":id,"label":label,"kind":kind,"help":"Provider sign-in is managed by this runtime node."})
}
fn provider(id: &str, label: &str, methods: Vec<Value>) -> Value {
    json!({"id":id,"label":label,"methods":methods})
}
pub fn descriptor(harness: &str) -> Result<Value> {
    descriptor_with(harness, &serde_json::json!({}))
}
pub(super) fn descriptor_with(harness: &str, config: &Value) -> Result<Value> {
    let mut descriptor = catalog(harness)?;
    if matches!(harness, "cursor" | "pi") && !super::native::sdk_auth_available_with(config) {
        for provider in descriptor["providers"].as_array_mut().unwrap() {
            provider["methods"]
                .as_array_mut()
                .unwrap()
                .retain(|method| !matches!(method["kind"].as_str(), Some("browser" | "device")));
        }
    }
    Ok(descriptor)
}
pub(super) fn catalog(harness: &str) -> Result<Value> {
    let mut providers = match harness {
        "codex" => vec![provider(
            "openai",
            "OpenAI",
            vec![
                existing(),
                login("device", "Sign in with ChatGPT", "device"),
                key(),
            ],
        )],
        "claude" => vec![provider(
            "anthropic",
            "Anthropic",
            vec![
                existing(),
                login("console", "Sign in to Anthropic Console", "browser"),
                key(),
            ],
        )],
        "cursor" => vec![provider(
            "cursor",
            "Cursor",
            vec![
                existing(),
                login("browser", "Sign in with Cursor", "browser"),
                key(),
            ],
        )],
        "grok" => vec![provider(
            "xai",
            "xAI",
            vec![
                existing(),
                login("device", "Sign in with xAI", "device"),
                key(),
            ],
        )],
        "antigravity" => vec![provider("google", "Google", {
            let mut methods = vec![existing(), key()];
            if cfg!(unix) {
                methods.insert(1, login("browser", "Sign in with Google", "browser"));
            }
            methods
        })],
        "pi" => {
            let mut providers = api_providers();
            providers.push(provider(
                "openai-codex",
                "ChatGPT",
                vec![
                    existing(),
                    login("browser", "Sign in with ChatGPT", "browser"),
                ],
            ));
            providers.push(provider(
                "github-copilot",
                "GitHub Copilot",
                vec![existing(), login("device", "Sign in with GitHub", "device")],
            ));
            providers
        }
        "opencode" => {
            let mut providers = api_providers();
            providers[0]["methods"]
                .as_array_mut()
                .unwrap()
                .insert(1, login("device", "Sign in with ChatGPT", "device"));
            providers.push(provider(
                "github-copilot",
                "GitHub Copilot",
                vec![existing(), login("device", "Sign in with GitHub", "device")],
            ));
            providers
        }
        id if ["gemini", "qwen", "hermes", "kiro"].contains(&id) => {
            vec![provider("native", "Native provider", vec![existing()])]
        }
        _ => {
            return Err(Error::Unsupported(
                "Unknown agent tool setup provider".into(),
            ));
        }
    };
    if matches!(harness, "cursor" | "pi") {
        for provider in &mut providers {
            for method in provider["methods"].as_array_mut().unwrap() {
                if matches!(method["kind"].as_str(), Some("browser" | "device")) {
                    method["help"] = json!(
                        "Requires the runtime authentication support assets and Node.js 22.13 or later."
                    );
                }
            }
        }
    }
    if harness == "cursor" {
        providers[0]["methods"][0]["label"] = json!("Use existing SDK credentials");
        providers[0]["methods"][0]["help"] = json!(
            "Reuse CURSOR_API_KEY on this runtime node. Cursor CLI sign-in does not authenticate the SDK Bridge."
        );
    }
    let message = (harness == "antigravity").then_some("Gemini API keys use the Gemini model provider. Google sign-in uses Antigravity's native credential store; disconnecting in Swarmie preserves that store.");
    let mut descriptor = json!({"providers":providers});
    if let Some(message) = message {
        descriptor["message"] = json!(message);
    }
    Ok(descriptor)
}
fn api_providers() -> Vec<Value> {
    [
        ("openai", "OpenAI"),
        ("anthropic", "Anthropic"),
        ("google", "Google Gemini"),
        ("xai", "xAI"),
        ("openrouter", "OpenRouter"),
    ]
    .into_iter()
    .map(|(id, label)| provider(id, label, vec![existing(), key()]))
    .collect()
}
pub fn validate_provider(harness: &str, provider: &str) -> Result<()> {
    let descriptor = catalog(harness)?;
    if descriptor["providers"]
        .as_array()
        .is_some_and(|providers| providers.iter().any(|item| item["id"] == provider))
    {
        Ok(())
    } else {
        Err(Error::Invalid(
            "Unsupported provider for this agent tool".into(),
        ))
    }
}
pub fn validate_method(harness: &str, provider: &str, method: &str) -> Result<()> {
    validate_provider(harness, provider)?;
    let descriptor = catalog(harness)?;
    if descriptor["providers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["id"] == provider)
        .unwrap()["methods"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item["id"] == method)
    {
        Ok(())
    } else {
        Err(Error::Unsupported(
            "Unsupported authentication method for this provider".into(),
        ))
    }
}
pub fn existing_overlay(harness: &str, provider: &str) -> Result<AgentToolExecutionOverlay> {
    validate_method(harness, provider, "existing")?;
    Ok(AgentToolExecutionOverlay {
        config: if matches!(harness, "pi" | "opencode") {
            json!({"provider":provider})
        } else {
            json!({})
        },
        ..Default::default()
    })
}
pub fn credential_overlay(
    harness: &str,
    provider: &str,
    key: &str,
    private_root: &Path,
) -> Result<AgentToolExecutionOverlay> {
    validate_method(harness, provider, "api_key")?;
    if key.is_empty() || key.len() > 8192 || key.contains(['\r', '\n', '\0']) {
        return Err(Error::Invalid("Invalid provider API key".into()));
    }
    let variable = match provider {
        "openai" => "OPENAI_API_KEY",
        "anthropic" => "ANTHROPIC_API_KEY",
        "google" => "GEMINI_API_KEY",
        "xai" => "XAI_API_KEY",
        "openrouter" => "OPENROUTER_API_KEY",
        "cursor" => "CURSOR_API_KEY",
        _ => {
            return Err(Error::Unsupported(
                "API keys are not supported for this provider".into(),
            ));
        }
    };
    let mut overlay = AgentToolExecutionOverlay {
        env: BTreeMap::from([(variable.into(), key.into())]),
        ..Default::default()
    };
    match harness {
        "codex" => {
            overlay.config = json!({"modelProvider":"swarmie-openai","config":{"model_providers.swarmie-openai":{"name":"OpenAI","base_url":"https://api.openai.com/v1","env_key":"OPENAI_API_KEY","wire_api":"responses","requires_openai_auth":false}}})
        }
        "pi" | "opencode" | "claude" | "grok" => {
            let home = api_profile_home(harness, provider, private_root).unwrap();
            std::fs::create_dir_all(&home)?;
            overlay.config_home = Some(home.clone());
            if matches!(harness, "pi" | "opencode") {
                overlay.config = json!({"provider":provider});
            }
            if harness == "opencode" {
                overlay.config["nativeAuthDataHome"] = json!(home.join("data"));
            }
            if harness == "claude" {
                overlay
                    .env
                    .insert("CLAUDE_CODE_USE_BEDROCK".into(), "0".into());
                overlay
                    .env
                    .insert("CLAUDE_CODE_USE_VERTEX".into(), "0".into());
                overlay
                    .env
                    .insert("CLAUDE_CODE_USE_FOUNDRY".into(), "0".into());
            }
        }
        "antigravity" => {
            let home = api_profile_home(harness, provider, private_root).unwrap();
            std::fs::create_dir_all(home.join(".gemini/antigravity-cli"))?;
            std::fs::write(
                home.join(".gemini/antigravity-cli/settings.json"),
                b"{\"modelProvider\":\"gemini\"}\n",
            )?;
            overlay.config_home = Some(home);
        }
        _ => (),
    }
    Ok(overlay)
}

fn api_profile_home(
    harness: &str,
    provider: &str,
    private_root: &Path,
) -> Option<std::path::PathBuf> {
    match harness {
        "pi" | "opencode" | "claude" | "grok" => Some(
            private_root
                .join("profiles")
                .join(harness)
                .join(format!("{provider}-api")),
        ),
        "antigravity" => Some(private_root.join("profiles/antigravity/gemini")),
        _ => None,
    }
}
pub(super) fn api_history_overlay(
    harness: &str,
    provider: &str,
    private_root: &Path,
) -> AgentToolExecutionOverlay {
    AgentToolExecutionOverlay {
        config_home: api_profile_home(harness, provider, private_root),
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn every_named_tool_has_an_existing_account_and_supported_provider_methods() {
        for harness in [
            "codex",
            "claude",
            "cursor",
            "grok",
            "opencode",
            "antigravity",
            "pi",
        ] {
            let descriptor = descriptor(harness).unwrap();
            assert!(!descriptor["providers"].as_array().unwrap().is_empty());
            for provider in descriptor["providers"].as_array().unwrap() {
                assert!(
                    provider["methods"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .any(|method| method["kind"] == "existing")
                );
            }
        }
        assert!(validate_method("cursor", "cursor", "device").is_err());
        assert!(validate_method("claude", "anthropic", "subscription").is_err());
    }
    #[test]
    fn api_overlay_contains_secrets_only_in_the_private_environment() {
        let root = std::env::temp_dir().join(format!("swarmie-provider-{}", uuid::Uuid::new_v4()));
        for (harness, provider) in [
            ("codex", "openai"),
            ("claude", "anthropic"),
            ("cursor", "cursor"),
            ("grok", "xai"),
            ("pi", "google"),
            ("opencode", "openai"),
            ("antigravity", "google"),
        ] {
            let overlay = credential_overlay(harness, provider, "fixture-secret", &root).unwrap();
            assert!(overlay.env.values().any(|value| value == "fixture-secret"));
            assert!(!overlay.config.to_string().contains("fixture-secret"));
        }
        assert!(
            !std::fs::read_to_string(
                root.join("profiles/antigravity/gemini/.gemini/antigravity-cli/settings.json")
            )
            .unwrap()
            .contains("fixture-secret")
        );
        std::fs::remove_dir_all(root).unwrap();
    }
}

#[cfg(test)]
mod profile_tests {
    use super::*;
    #[test]
    fn managed_keys_isolate_tools_whose_native_store_overrides_environment() {
        let root =
            std::env::temp_dir().join(format!("swarmie-auth-profiles-{}", uuid::Uuid::new_v4()));
        for (harness, provider, variable) in [
            ("grok", "xai", "GROK_HOME"),
            ("pi", "anthropic", "PI_CODING_AGENT_DIR"),
            ("claude", "anthropic", "CLAUDE_CONFIG_DIR"),
        ] {
            let overlay =
                credential_overlay(harness, provider, "fixture-private-api-key", &root).unwrap();
            let home = overlay.config_home.as_ref().unwrap();
            assert!(home.starts_with(&root));
            assert!(std::fs::read_dir(home).unwrap().next().is_none());
            let mut options = swarmie::SpawnOptions::new(harness, &root);
            options.execution.config_home = overlay.config_home;
            options.execution.env = overlay.env;
            let environment = crate::execution_env(&options, Some(variable)).unwrap();
            assert!(environment[variable].starts_with(&root.to_string_lossy().into_owned()));
            assert!(
                environment
                    .values()
                    .any(|value| value == "fixture-private-api-key")
            );
        }
        let codex =
            credential_overlay("codex", "openai", "fixture-private-api-key", &root).unwrap();
        assert_eq!(codex.config["modelProvider"], "swarmie-openai");
        assert_eq!(
            codex.config["config"]["model_providers.swarmie-openai"]["env_key"],
            "OPENAI_API_KEY"
        );
        assert_eq!(
            codex.config["config"]["model_providers.swarmie-openai"]["requires_openai_auth"],
            false
        );
        std::fs::remove_dir_all(root).unwrap();
    }
}
