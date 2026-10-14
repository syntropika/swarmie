// Copyright 2026 Syntropika
// SPDX-License-Identifier: Apache-2.0
//! Typed CLI entry points share the native owner with desktop clients.
use super::{call, serve};
use serde_json::json;
use std::path::{Path, PathBuf};
use swarmie::{Error, Result};
#[derive(clap::Subcommand)]
pub enum AccountCommand {
    /// Run the persistent credential owner independently of execution nodes and UI windows.
    Serve,
    /// Read and revalidate the current account through its native owner.
    Read,
    /// Select a first-party account origin. Only permitted while signed out.
    Configure { origin: String },
    /// Start browser sign-in; session-only use must be explicitly requested.
    Login {
        #[arg(long)]
        session_only: bool,
        #[arg(long)]
        no_open: bool,
    },
    /// Cancel a pending login using its native identifier.
    Cancel { login_id: uuid::Uuid },
    /// Clear local credentials and attempt remote grant revocation.
    Logout,
    /// List this account's active native sessions, with bounded pagination.
    Sessions {
        #[arg(long)]
        after: Option<uuid::Uuid>,
    },
    /// Revoke one native session belonging to the current account.
    RevokeSession { session_id: uuid::Uuid },
    /// Revoke every native session, including this installation and pending approved codes.
    LogoutAll,
    /// Save an app-owned profile name with revision and receipt checks.
    Profile {
        name: String,
        #[arg(long)]
        revision: i64,
        #[arg(long)]
        request_id: Option<uuid::Uuid>,
    },
    /// Upload or remove your app-owned profile image.
    Avatar {
        #[arg(long, conflicts_with = "remove", required_unless_present = "remove")]
        path: Option<PathBuf>,
        #[arg(long)]
        remove: bool,
        #[arg(long)]
        revision: i64,
        #[arg(long)]
        request_id: Option<uuid::Uuid>,
    },
    /// Stop the account owner; secure grants can be restored and revalidated on restart.
    Stop,
}
pub async fn run(root: &Path, command: AccountCommand) -> Result<()> {
    if matches!(command, AccountCommand::Serve) {
        return serve(root).await;
    }
    let (method, params) = match command {
        AccountCommand::Read => ("account/read", json!({})),
        AccountCommand::Configure { origin } => ("account/configure", json!({"origin":origin})),
        AccountCommand::Login {
            session_only,
            no_open,
        } => (
            "account/login/start",
            json!({"sessionOnly":session_only,"openBrowser":!no_open}),
        ),
        AccountCommand::Cancel { login_id } => {
            ("account/login/cancel", json!({"loginId":login_id}))
        }
        AccountCommand::Logout => ("account/logout", json!({})),
        AccountCommand::Sessions { after } => {
            let account = account_id(root).await?;
            (
                "account/sessions/read",
                json!({"expectedAccountId":account,"after":after}),
            )
        }
        AccountCommand::RevokeSession { session_id } => {
            let account = account_id(root).await?;
            (
                "account/sessions/revoke",
                json!({"expectedAccountId":account,"sessionId":session_id}),
            )
        }
        AccountCommand::LogoutAll => {
            let account = account_id(root).await?;
            (
                "account/sessions/revoke-all",
                json!({"expectedAccountId":account}),
            )
        }
        AccountCommand::Profile {
            name,
            revision,
            request_id,
        } => {
            let current = call(
                root,
                json!({"jsonrpc":"2.0","id":2,"method":"account/read","params":{}}),
            )
            .await?;
            (
                "account/profile/update",
                json!({"accountId":current["result"]["profile"]["id"],"displayName":name,"expectedRevision":revision,"requestId":request_id.unwrap_or_else(uuid::Uuid::now_v7)}),
            )
        }
        AccountCommand::Avatar {
            path,
            remove: _,
            revision,
            request_id,
        } => {
            use base64::Engine;
            use tokio::io::AsyncReadExt;
            let image = if let Some(path) = path {
                let mut bytes = Vec::new();
                tokio::fs::File::open(path)
                    .await?
                    .take(1024 * 1024 + 1)
                    .read_to_end(&mut bytes)
                    .await?;
                if bytes.len() > 1024 * 1024 {
                    return Err(Error::Invalid("avatar must be under 1 MiB".into()));
                }
                Some(base64::engine::general_purpose::STANDARD.encode(bytes))
            } else {
                None
            };
            let current = call(
                root,
                json!({"jsonrpc":"2.0","id":2,"method":"account/read","params":{}}),
            )
            .await?;
            (
                "account/profile/avatar/update",
                json!({"accountId":current["result"]["profile"]["id"],"imageBase64":image,"expectedRevision":revision,"requestId":request_id.unwrap_or_else(uuid::Uuid::now_v7)}),
            )
        }
        AccountCommand::Stop => ("account/owner/stop", json!({})),
        AccountCommand::Serve => unreachable!(),
    };
    let response = call(
        root,
        json!({"jsonrpc":"2.0","id":1,"method":method,"params":params}),
    )
    .await?;
    if response.get("error").is_some() {
        return Err(Error::Provider(
            response["error"]["message"]
                .as_str()
                .unwrap_or("account request failed")
                .into(),
        ));
    }
    println!("{}", serde_json::to_string_pretty(&response["result"])?);
    Ok(())
}
async fn account_id(root: &Path) -> Result<uuid::Uuid> {
    let current = call(
        root,
        json!({"jsonrpc":"2.0","id":2,"method":"account/read","params":{}}),
    )
    .await?;
    current["result"]["profile"]["id"]
        .as_str()
        .and_then(|value| uuid::Uuid::parse_str(value).ok())
        .ok_or_else(|| Error::Invalid("sign in before managing account sessions".into()))
}
