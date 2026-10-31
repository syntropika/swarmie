//! Reference-only model-facing tool metadata. Runtime admission revalidates every field.
use serde_json::{Value, json};
use swarmie_tools::{ToolDefinition, ToolName, ToolProvider, ToolSpec};

pub fn definitions() -> tembra::Result<Vec<ToolDefinition>> {
    let uuid = json!({"type":"string","format":"uuid"});
    let reference = json!({"type":"object","additionalProperties":false,"required":["account","vault","secret"],"properties":{"account":uuid,"vault":uuid,"secret":uuid}});
    let scope = json!({"oneOf":[{"type":"object","additionalProperties":false,"required":["kind"],"properties":{"kind":{"const":"unrestricted"}}},{"type":"object","additionalProperties":false,"required":["kind","ids"],"properties":{"kind":{"const":"only"},"ids":{"type":"array","maxItems":256,"uniqueItems":true,"items":uuid}}}]});
    let metadata = json!({"type":"object","additionalProperties":false,"required":["name","purpose","projects","agents","approval","policy_generation","target_account"],"properties":{"name":{"type":"string","minLength":1,"maxLength":256},"purpose":{"type":"string","maxLength":4096},"projects":scope,"agents":scope,"approval":{"enum":["none","operation","conversation"]},"policy_generation":uuid,"target_account":{"type":["string","null"],"maxLength":1024}}});
    let timeout = json!({"type":"integer","minimum":1,"maximum":300000});
    let bash = json!({"type":"object","additionalProperties":false,"required":["source","cwd","environment","timeout_ms"],"properties":{"source":{"type":"string","description":"Unmodified Bash source. Read credentials through the declared SWARMIE_SECRET_* environment variables; never insert a placeholder into shell source."},"cwd":{"type":"string"},"environment":{"type":"object","additionalProperties":reference},"timeout_ms":timeout}});
    let http = json!({"type":"object","additionalProperties":false,"required":["url","method","body","credentials","fields","timeout_ms"],"properties":{"url":{"type":"string","description":"Absolute HTTPS URL without credentials or secret placeholders. Redirects and automatic retries are disabled."},"method":{"type":"string"},"body":{},"credentials":{"type":"object","additionalProperties":{"type":"object","additionalProperties":false,"required":["reference","prefix"],"properties":{"reference":reference,"prefix":{"type":"string","description":"For example Bearer followed by one space."}}}},"fields":{"type":"array","items":{"type":"object","additionalProperties":false,"required":["pointer","reference"],"properties":{"pointer":{"type":"string","description":"JSON pointer selecting an exact reference placeholder leaf in body."},"reference":reference}}},"timeout_ms":timeout}});
    let source = json!({"oneOf":[{"type":"object","additionalProperties":false,"required":["kind","bytes"],"properties":{"kind":{"const":"random"},"bytes":{"type":"integer","minimum":16,"maximum":4096}}},{"type":"object","additionalProperties":false,"required":["kind","request","pointer"],"properties":{"kind":{"const":"bash_json"},"request":bash,"pointer":{"type":"string","maxLength":1024}}},{"type":"object","additionalProperties":false,"required":["kind","request","pointer"],"properties":{"kind":{"const":"http_json"},"request":http,"pointer":{"type":"string","maxLength":1024}}}]});
    let input = json!({"type":"object","additionalProperties":false,"required":["metadata"],"properties":{"secret":uuid,"expected":{"type":["string","null"],"format":"uuid"},"metadata":metadata}});
    let mut capture = input.clone();
    capture["properties"]["source"] = source;
    capture["required"] = json!(["metadata", "source"]);
    let entries: [(&str, &str, Value); 5] = [
        (
            "vault_list",
            "List eligible secret names and reference metadata for this actual execution. This never returns values.",
            json!({"type":"object","additionalProperties":false,"properties":{}}),
        ),
        (
            "vault_request_input",
            "Ask the user to enter a secret through a private chat card. Do not ask them to paste credentials into an ordinary message. Return only a save receipt/reference. Replacements must preserve the existing policy and scopes.",
            input,
        ),
        (
            "vault_capture",
            "Request a human-approved native random, HTTPS JSON or Bash JSON secret capture. The selected string is sealed privately; all Bash output is withheld. Unknown issuance must be reconciled, never automatically repeated.",
            capture,
        ),
        (
            "vault_bash",
            "Run bounded Bash with eligible secrets delivered as declared environment data. The returned output masks exact same-call reflections. Effects may be uncertain; do not assume a failed command can be replayed safely.",
            bash,
        ),
        (
            "vault_http",
            "Send a bounded HTTPS request with eligible secrets injected into declared headers or exact JSON leaves. The returned response masks exact same-call reflections. Effects may be uncertain; do not automatically replay issuance.",
            http,
        ),
    ];
    entries
        .into_iter()
        .map(|(name, description, input_schema)| {
            ToolDefinition::new(
                ToolSpec {
                    name: ToolName::new(name)?,
                    description: description.into(),
                    input_schema,
                },
                ToolProvider::new(crate::tools::SERVICE, vec!["main".into()])?,
            )
        })
        .collect()
}
