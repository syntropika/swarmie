use crate::ai::Provider;

pub(super) struct ProviderSpec {
    pub id: &'static str,
    pub provider: Provider,
    pub endpoint: &'static str,
    pub catalog_provider: Option<&'static str>,
}

pub(super) fn specifications() -> [ProviderSpec; 4] {
    [
        ProviderSpec {
            id: "openai",
            provider: Provider::OpenAi,
            endpoint: "https://api.openai.com/v1",
            catalog_provider: Some("openai"),
        },
        ProviderSpec {
            id: "anthropic",
            provider: Provider::Anthropic,
            endpoint: "https://api.anthropic.com/v1",
            catalog_provider: Some("anthropic"),
        },
        ProviderSpec {
            id: "kimi",
            provider: Provider::Kimi,
            endpoint: "https://api.moonshot.ai/v1",
            catalog_provider: Some("moonshotai"),
        },
        ProviderSpec {
            id: "ollama",
            provider: Provider::Ollama,
            endpoint: "http://localhost:11434",
            catalog_provider: None,
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_product_choice_has_one_native_adapter() {
        let choices = crate::settings::provider_catalog();
        let specs = specifications();
        assert_eq!(choices.len(), specs.len());
        for choice in choices {
            assert_eq!(specs.iter().filter(|spec| spec.id == choice.id).count(), 1);
            assert!(!choice.methods.is_empty());
        }
        let kimi = specs.iter().find(|spec| spec.id == "kimi").unwrap();
        assert_eq!(kimi.provider, Provider::Kimi);
    }
}
