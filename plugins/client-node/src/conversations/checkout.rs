//! Checkout admission and per-session Tools guards over the owning capability.
use super::{failure, types::ExecutionContext};
use futures::{FutureExt, future::LocalBoxFuture};
use std::rc::Rc;
use swarmie_tools::{
    CatalogService, Recovery, ToolCall, ToolDecision, ToolExecution, ToolPreparation, ToolResult,
    ToolSpec, Tools,
};
use tembra::Context;

pub(super) async fn validate(
    git: Option<&swarmie_git::GitService>,
    context: &Context,
    node: &str,
    principal: &str,
    binding: &ExecutionContext,
) -> tembra::Result<()> {
    context.check()?;
    if binding.version != 1
        || binding.owner != "git"
        || binding.node_id != node
        || binding.principal != principal
        || binding.path.is_empty()
        || binding.path.len() > 4096
        || binding.identity.len() > 16 * 1024
        || serde_json::to_vec(binding).map_or(true, |bytes| bytes.len() > 8 * 1024)
    {
        return Err(failure("the checkout belongs to another node or principal"));
    }
    let git = git
        .ok_or_else(|| failure("Git is unavailable on the captured execution node"))?
        .clone();
    let principal = principal.to_owned();
    let path = binding.path.clone();
    let response = tokio::task::spawn_blocking(move || {
        git.git().execute(
            &principal,
            swarmie_git::Request::Identify { path },
            &swarmie_git::Cancellation::default(),
        )
    })
    .await
    .map_err(|_| failure("checkout validation did not settle"))?
    .map_err(|error| failure(&error.to_string()))?;
    context.check()?;
    let swarmie_git::Response::Repository {
        repository: Some(repository),
    } = response
    else {
        return Err(failure("the captured Git checkout is unavailable"));
    };
    let identity = serde_json::to_string(&(
        &repository.root,
        &repository.git_dir,
        &repository.common_dir,
    ))
    .map_err(|_| failure("the checkout identity could not be encoded"))?;
    if repository.bare
        || repository.root.as_deref() != Some(binding.path.as_str())
        || identity != binding.identity
    {
        return Err(failure(
            "the checkout identity changed; start a new conversation",
        ));
    }
    Ok(())
}

/// A request-scoped guard; the owner CatalogService still resolves and confirms providers.
pub(super) struct ScopedTools {
    pub inner: Rc<CatalogService>,
    pub binding: Option<ExecutionContext>,
    pub authority: Option<std::sync::Arc<dyn swarmie_git::Authority>>,
}

impl ScopedTools {
    fn allows(&self, call: &ToolCall) -> tembra::Result<()> {
        allows(self.binding.as_ref(), call)?;
        if call.name.as_str() == "git_mutate" {
            #[derive(serde::Deserialize)]
            #[serde(deny_unknown_fields)]
            struct Mutation {
                target: swarmie_git::RepositoryTarget,
                operation: swarmie_git::Operation,
            }
            let mutation: Mutation = serde_json::from_value(call.arguments.clone())
                .map_err(|_| failure("the Git mutation is invalid"))?;
            let authority = self
                .authority
                .as_ref()
                .ok_or_else(|| failure("agent Git mutations require an explicit host policy"))?;
            let binding = self
                .binding
                .as_ref()
                .ok_or_else(|| failure("the conversation checkout is unavailable"))?;
            authority
                .authorize(
                    &binding.principal,
                    &swarmie_git::Intent {
                        target: mutation.target,
                        operation: swarmie_git::IntentOperation::Mutate {
                            operation: mutation.operation,
                        },
                    },
                )
                .map_err(|error| failure(&error.to_string()))?;
        }
        Ok(())
    }
}

fn allows(binding: Option<&ExecutionContext>, call: &ToolCall) -> tembra::Result<()> {
    if !matches!(call.name.as_str(), "git_inspect" | "git_mutate") {
        return Ok(());
    }
    let binding = binding.ok_or_else(|| failure("this conversation has no captured checkout"))?;
    let value = &call.arguments;
    if call.name.as_str() == "git_inspect"
        && matches!(value["kind"].as_str(), Some("discover" | "identify"))
    {
        if value["path"].as_str() != Some(binding.path.as_str()) {
            return Err(failure("Git discovery is limited to the captured checkout"));
        }
        return Ok(());
    }
    let target = &value["target"];
    let identity = serde_json::to_string(&(
        target.get("root"),
        target.get("git_dir"),
        target.get("common_dir"),
    ))
    .map_err(|_| failure("the Git target could not be encoded"))?;
    if identity != binding.identity || target["bare"] != false {
        return Err(failure("the Git tool targets another checkout"));
    }
    Ok(())
}

impl Tools for ScopedTools {
    fn validate<'a>(
        &'a self,
        context: &'a Context,
        spec: &'a ToolSpec,
        call: &'a ToolCall,
    ) -> LocalBoxFuture<'a, tembra::Result<ToolDecision>> {
        async move { Ok(self.prepare(context, spec, call).await?.decision) }.boxed_local()
    }
    fn prepare<'a>(
        &'a self,
        context: &'a Context,
        spec: &'a ToolSpec,
        call: &'a ToolCall,
    ) -> LocalBoxFuture<'a, tembra::Result<ToolPreparation>> {
        async move {
            if let Err(error) = self.allows(call) {
                return Ok(ToolPreparation {
                    decision: ToolDecision::Block(error.message),
                    provider: None,
                });
            }
            self.inner.prepare(context, spec, call).await
        }
        .boxed_local()
    }
    fn prepare_retry<'a>(
        &'a self,
        context: &'a Context,
        spec: &'a ToolSpec,
        execution: ToolExecution<'a>,
    ) -> LocalBoxFuture<'a, tembra::Result<ToolPreparation>> {
        async move {
            if let Err(error) = self.allows(execution.call) {
                return Ok(ToolPreparation {
                    decision: ToolDecision::Block(error.message),
                    provider: execution.provider.cloned(),
                });
            }
            self.inner.prepare_retry(context, spec, execution).await
        }
        .boxed_local()
    }
    fn execute<'a>(
        &'a self,
        context: &'a Context,
        execution: ToolExecution<'a>,
    ) -> LocalBoxFuture<'a, tembra::Result<ToolResult>> {
        async move {
            if let Err(error) = self.allows(execution.call) {
                return Ok(ToolResult::Blocked(error.message));
            }
            self.inner.execute(context, execution).await
        }
        .boxed_local()
    }
    fn recover<'a>(
        &'a self,
        context: &'a Context,
        execution: ToolExecution<'a>,
    ) -> LocalBoxFuture<'a, tembra::Result<Recovery<ToolResult>>> {
        async move {
            self.allows(execution.call)?;
            self.inner.recover(context, execution).await
        }
        .boxed_local()
    }
}

pub(super) fn bounded_warning(mut text: String) -> String {
    const LIMIT: usize = 2048;
    const MARKER: &str = " [warning truncated; inspect the retained Git operation]";
    if text.len() > LIMIT {
        let mut end = LIMIT - MARKER.len();
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        text.truncate(end);
        text.push_str(MARKER);
    }
    text
}

/// Delivery failure is presentation evidence, never a replacement execution outcome.
pub(super) async fn sync_hold(
    index: &super::index::Index,
    git: Option<&swarmie_git::GitService>,
    principal: &str,
    id: &str,
) {
    let _delivery = index.lifecycle.lock().await;
    let Ok(records) = index.read() else {
        return;
    };
    let Some(binding) = records
        .iter()
        .find(|record| record.id == id)
        .and_then(|record| record.execution_context.clone())
    else {
        return;
    };
    let related: Vec<_> = records
        .into_iter()
        .filter(|record| {
            record.principal == principal
                && record.node == binding.node_id
                && record.execution_context.as_ref().is_some_and(|context| {
                    context.node_id == binding.node_id
                        && context.principal == binding.principal
                        && context.identity == binding.identity
                })
        })
        .collect();
    // The conversation index is the owner of known writers. A model cannot begin
    // dispatch until this delivery finishes, including when another chat archives.
    let active = related.iter().any(|record| {
        matches!(
            record.phase,
            super::types::Phase::Working | super::types::Phase::Unknown
        )
    });
    let ids: Vec<_> = related.iter().map(|record| record.id.clone()).collect();
    let result = match git {
        None => Err("Git retention is unavailable on the captured node".into()),
        Some(git) => {
            let git = git.clone();
            let principal = principal.to_owned();
            tokio::task::spawn_blocking(move || {
                sync_checkout(&git, &principal, binding, related, active)
            })
            .await
            .map_err(|_| "Git retention delivery did not settle".to_owned())
            .and_then(|result| result)
        }
    };
    let warnings = match result {
        Ok(warnings) => warnings,
        Err(error) => ids
            .into_iter()
            .map(|id| (id, Some(error.clone())))
            .collect(),
    };
    for (id, warning) in warnings {
        let warning = warning.map(bounded_warning);
        let _ = index.change(&id, |record| record.lifecycle_warning = warning);
    }
}

fn sync_checkout(
    git: &swarmie_git::GitService,
    principal: &str,
    binding: ExecutionContext,
    records: Vec<super::types::Metadata>,
    active: bool,
) -> Result<Vec<(String, Option<String>)>, String> {
    let cancellation = swarmie_git::Cancellation::default();
    let response = git
        .git()
        .execute(
            principal,
            swarmie_git::Request::Identify { path: binding.path },
            &cancellation,
        )
        .map_err(|error| error.to_string())?;
    let swarmie_git::Response::Repository {
        repository: Some(target),
    } = response
    else {
        return Err("the captured checkout is unavailable for retention".into());
    };
    let identity = serde_json::to_string(&(&target.root, &target.git_dir, &target.common_dir))
        .map_err(|error| error.to_string())?;
    if identity != binding.identity {
        return Err("the captured checkout changed; its previous retention state was kept".into());
    }
    Ok(records
        .into_iter()
        .map(|record| {
            let id = record.id;
            let warning = match git.set_conversation_hold(
                principal,
                &target,
                swarmie_git::ConversationHoldUpdate {
                    conversation_id: id.clone(),
                    pinned: record.pinned,
                    archived: record.archived,
                    active,
                },
                &cancellation,
            ) {
                Ok(hold) => hold.warning,
                Err(error) => Some(error.to_string()),
            };
            (id, warning)
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use swarmie_tools::{ToolCallId, ToolName};

    #[test]
    fn scope_fences_git_targets_without_rewriting_other_provider_resources() {
        let binding = ExecutionContext {
            version: 1,
            node_id: "fixture-node".into(),
            principal: "fixture-user".into(),
            path: "/project".into(),
            owner: "git".into(),
            identity: serde_json::to_string(&json!(["/project", "/project/.git", "/project/.git"]))
                .unwrap(),
        };
        let call = |name: &str, arguments| ToolCall {
            id: ToolCallId::new("fixture-call").unwrap(),
            name: ToolName::new(name).unwrap(),
            arguments,
        };
        let target = json!({"root":"/project", "git_dir":"/project/.git", "common_dir":"/project/.git", "bare":false});
        assert!(
            allows(
                Some(&binding),
                &call("git_inspect", json!({"kind":"status", "target":target}))
            )
            .is_ok()
        );
        assert!(
            allows(
                None,
                &call("git_inspect", json!({"kind":"status", "target":target}))
            )
            .is_err()
        );
        let mut changed = target.clone();
        changed["git_dir"] = json!("/project/.git/worktrees/other");
        assert!(
            allows(
                Some(&binding),
                &call(
                    "git_mutate",
                    json!({"target":changed, "operation":{"kind":"stage","paths":["owned.rs"]}})
                )
            )
            .is_err()
        );
        assert!(
            allows(
                Some(&binding),
                &call("git_inspect", json!({"kind":"discover", "path":"/other"}))
            )
            .is_err()
        );
        assert!(
            allows(
                Some(&binding),
                &call("git_inspect", json!({"kind":"discover", "path":"/project"}))
            )
            .is_ok()
        );
        assert!(
            allows(
                Some(&binding),
                &call(
                    "mcp.documents.read",
                    json!({"resource":"provider-owned-resource"})
                )
            )
            .is_ok()
        );
    }
}
