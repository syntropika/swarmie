use serde::{Deserialize, Serialize};

use super::request::CompletionRequest;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SystemPrompt {
    pub role: String,
    pub sections: Vec<Section>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Section {
    pub tag: String,
    pub text: String,
}

impl Section {
    pub fn new(tag: impl Into<String>, text: impl Into<String>) -> Self {
        Self {
            tag: tag.into(),
            text: text.into(),
        }
    }
}

impl SystemPrompt {
    pub fn text(&self) -> String {
        let mut prompt = self.role.trim().to_owned();
        for section in &self.sections {
            let text = section.text.trim();
            if text.is_empty() {
                continue;
            }
            if !prompt.is_empty() {
                prompt.push_str("\n\n");
            }
            let tag = &section.tag;
            prompt.push_str(&format!("<{tag}>\n{text}\n</{tag}>"));
        }
        prompt
    }

    pub fn section(&self, tag: &str) -> Option<&Section> {
        self.sections.iter().find(|section| section.tag == tag)
    }

    pub fn take(&mut self, tag: &str) -> Option<Section> {
        let at = self
            .sections
            .iter()
            .position(|section| section.tag == tag)?;
        Some(self.sections.remove(at))
    }
}

impl CompletionRequest {
    #[must_use]
    pub fn with_system(self, prompt: &SystemPrompt) -> Self {
        self.preamble(prompt.text())
    }
}
