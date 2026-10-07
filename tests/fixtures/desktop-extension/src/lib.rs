use async_trait::async_trait;
use mpai::{Error, ExtensionCommand, ExtensionView, Result, RuntimeExtension};
use serde_json::{Value, json};
use std::path::PathBuf;

/// A third-party web view using only the public extension contract.
pub struct HtmlExtension {
    html_path: PathBuf,
}

impl HtmlExtension {
    pub fn new(html_path: impl Into<PathBuf>) -> Self {
        Self {
            html_path: html_path.into(),
        }
    }
}

#[async_trait]
impl RuntimeExtension for HtmlExtension {
    fn id(&self) -> &str {
        "external-html-fixture"
    }

    fn commands(&self) -> Vec<ExtensionCommand> {
        [
            ("view.html", "Load HTML from disk"),
            ("fixture.echo", "Echo an extension command"),
            ("fixture.receipt", "Record frame acceptance evidence"),
        ]
        .into_iter()
        .map(|(name, title)| ExtensionCommand {
            name: name.into(),
            title: title.into(),
            mutates: false,
        })
        .collect()
    }

    fn views(&self) -> Vec<ExtensionView> {
        vec![ExtensionView {
            id: "external-html".into(),
            title: "External HTML fixture".into(),
            kind: "web".into(),
            command: "view.html".into(),
        }]
    }

    async fn invoke(&self, command: &str, arguments: Value) -> Result<Value> {
        match command {
            "view.html" => {
                // Deliberately uncached: Reload view reads the author's current file.
                let html = tokio::fs::read_to_string(&self.html_path).await?;
                eprintln!("FIXTURE_VIEW_LOADED {} bytes", html.len());
                Ok(json!({ "html": html }))
            }
            "fixture.echo" => {
                let message = arguments["message"]
                    .as_str()
                    .ok_or_else(|| Error::Invalid("message must be a string".into()))?;
                Ok(json!({ "extensionId": self.id(), "message": message, "roundtrip": true }))
            }
            "fixture.receipt" => {
                eprintln!("FIXTURE_FRAME_RECEIPT {arguments}");
                Ok(json!({ "recorded": true }))
            }
            _ => Err(Error::Unsupported(format!("extension command {command}"))),
        }
    }
}
