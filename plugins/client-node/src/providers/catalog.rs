use swarmie_model_provider_genai::Provider;
use swarmie_providers::settings::Choice;

pub(super) struct ProviderSpec {
    pub id: &'static str,
    pub provider: Provider,
    pub endpoint: &'static str,
}

pub(super) fn specifications() -> [ProviderSpec; 4] {
    [
        ProviderSpec {
            id: "openai",
            provider: Provider::OpenAi,
            endpoint: "https://api.openai.com/v1",
        },
        ProviderSpec {
            id: "anthropic",
            provider: Provider::Anthropic,
            endpoint: "https://api.anthropic.com/v1",
        },
        ProviderSpec {
            id: "kimi",
            provider: Provider::OpenAi,
            endpoint: "https://api.moonshot.ai/v1",
        },
        ProviderSpec {
            id: "ollama",
            provider: Provider::Ollama,
            endpoint: "http://localhost:11434",
        },
    ]
}

pub(super) fn choices() -> Vec<Choice> {
    swarmie_providers::settings::provider_catalog()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_product_choice_has_one_native_adapter() {
        let choices = choices();
        let specs = specifications();
        assert_eq!(choices.len(), specs.len());
        for choice in choices {
            assert_eq!(specs.iter().filter(|spec| spec.id == choice.id).count(), 1);
            assert!(!choice.methods.is_empty());
        }
        let kimi = specs.iter().find(|spec| spec.id == "kimi").unwrap();
        assert_eq!(kimi.provider, Provider::OpenAi);
    }
}
