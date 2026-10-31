//! Native project acquisition and per-call reads through the owning Tembra session.
use super::{Session, checkout, failure, types::ExecutionContext};
use serde::Deserialize;
use serde_json::json;
use swarmie_context::environment::{Configuration, Environment, Inventory, Scope, Selection};
use swarmie_providers::{ContextSources, SourceKind};
use swarmie_tools::{ToolCall, ToolDecision, ToolProvider, ToolResult, ToolSpec};
use tembra::{Context, Host};

pub(super) const TOOL: &str = "skill_read";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Read {
    id: String,
    revision: String,
    path: String,
}

pub(super) fn tool_spec() -> tembra::Result<ToolSpec> {
    Ok(ToolSpec { name: swarmie_tools::ToolName::new(TOOL)?, description: "Read the instructions or a UTF-8 supporting file of an advertised skill. Use its exact id and revision. Paths are relative to that skill package. Reading does not execute code or grant permissions.".into(), input_schema: json!({"type":"object","additionalProperties":false,"properties":{"id":{"type":"string"},"revision":{"type":"string"},"path":{"type":"string"}},"required":["id","revision","path"]}) })
}

fn open(binding: &ExecutionContext, configuration: Configuration) -> tembra::Result<Environment> {
    Environment::new(
        Scope::new(&binding.path, &binding.path).map_err(source_error)?,
        configuration,
    )
    .map_err(source_error)
}
fn source_error(error: swarmie_context::environment::Error) -> tembra::Error {
    tembra::Error::invalid("swarmie.context.source", error.to_string())
}

pub(super) fn with_sources(
    request: swarmie_agent_loop::conversation::Request,
    sources: Option<ContextSources>,
) -> tembra::Result<swarmie_agent_loop::conversation::Request> {
    match sources {
        Some(sources) => request.with_sources(sources),
        None => Ok(request),
    }
}

impl Session {
    pub(super) async fn captured_sources(
        &self,
        context: &Context,
        metadata: &super::types::Metadata,
    ) -> tembra::Result<Option<ContextSources>> {
        let Some(id) = metadata.task else {
            return Ok(metadata
                .checkpoint
                .as_ref()
                .and_then(|checkpoint| checkpoint.sources().cloned()));
        };
        let retained = self
            .tasks
            .observe(context, id)
            .await?
            .ok_or_else(|| failure("the admitted turn capture is unavailable"))?;
        if retained.task.principal != self.principal
            || retained.task.spec.definition != super::tasks::definition()
        {
            return Err(failure(
                "the retained source capture belongs to another owner",
            ));
        }
        let input: super::tasks::Input = serde_json::from_value(retained.task.spec.input)
            .map_err(|_| failure("the retained source input is invalid"))?;
        if input.session != self.id || input.binding != metadata.execution_context {
            return Err(failure(
                "the retained source input targets another checkout",
            ));
        }
        Ok(input.request.sources)
    }

    pub(super) fn context_tools(
        &self,
        binding: Option<&ExecutionContext>,
        sources: Option<&ContextSources>,
    ) -> tembra::Result<Vec<ToolSpec>> {
        let mut tools = self
            .catalog
            .snapshot()?
            .specs()
            .into_iter()
            .filter(|spec| {
                binding.is_some() || !matches!(spec.name.as_str(), "git_inspect" | "git_mutate")
            })
            .collect::<Vec<_>>();
        if sources.is_some_and(|sources| {
            sources.parts().iter().any(|part| {
                matches!(
                    part.kind(),
                    SourceKind::SkillCatalog | SourceKind::SkillInstructions
                )
            })
        }) {
            tools.push(tool_spec()?);
        }
        Ok(tools)
    }

    pub(super) async fn capture_sources(
        &self,
        context: &Context,
        binding: Option<&ExecutionContext>,
        text: &str,
    ) -> tembra::Result<Option<ContextSources>> {
        let (Some(configuration), Some(binding)) = (&self.environment_configuration, binding)
        else {
            if text.trim_start().starts_with("/skill:") {
                return Err(failure(
                    "skill use requires an enabled project capability and a captured checkout",
                ));
            }
            return Ok(None);
        };
        context.check()?;
        let binding = binding.clone();
        let configuration = configuration.clone();
        let node = self.node.clone();
        let principal = self.principal.clone();
        let text = text.to_owned();
        let sources = tokio::task::spawn_blocking(move || {
            let project = open(&binding, configuration)?;
            let mut selected = Vec::new();
            if let Some(command) = text.trim_start().strip_prefix("/skill:") {
                let token = command
                    .split_whitespace()
                    .next()
                    .ok_or_else(|| failure("select a skill name or exact id@revision"))?;
                selected.push(
                    project
                        .inspect()
                        .map_err(source_error)?
                        .select(token)
                        .map_err(source_error)?,
                );
            }
            project
                .capture(&node, &principal, &selected)
                .map_err(source_error)
        })
        .await
        .map_err(|_| failure("project source capture did not settle"))??;
        context.check()?;
        Ok(Some(sources))
    }

    pub(super) async fn environment_inventory(
        &self,
        context: &Context,
        binding: &ExecutionContext,
    ) -> tembra::Result<Inventory> {
        context.check()?;
        if let Some(metadata) = self.index.get(&self.id)?
            && metadata.execution_context.as_ref() != Some(binding)
        {
            return Err(failure("the catalog targets another conversation checkout"));
        }
        checkout::validate(
            self.git.as_ref(),
            context,
            &self.node,
            &self.principal,
            binding,
        )
        .await?;
        let configuration = self
            .environment_configuration
            .clone()
            .ok_or_else(|| failure("project content is disabled"))?;
        let binding = binding.clone();
        let inventory = tokio::task::spawn_blocking(move || {
            open(&binding, configuration)?
                .inspect()
                .map_err(source_error)
        })
        .await
        .map_err(|_| failure("skill discovery did not settle"))??;
        context.check()?;
        if serde_json::to_vec(&inventory)
            .map_err(|_| failure("skill catalog cannot be encoded"))?
            .len()
            > swarmie_providers::MAX_CONTEXT_SOURCE_BYTES
        {
            return Err(failure("skill catalog exceeds the native response budget"));
        }
        Ok(inventory)
    }

    pub(super) fn authorize_skill(&self, context: &Context, call: &ToolCall) -> tembra::Result<()> {
        context.check()?;
        if context.principal()? != self.principal
            || call.name.as_str() != TOOL
            || self.environment_configuration.is_none()
        {
            return Err(failure("skill read is not authorized"));
        }
        let args: Read = serde_json::from_value(call.arguments.clone())
            .map_err(|_| failure("invalid skill read arguments"))?;
        let running = self.running.borrow();
        let running = running
            .as_ref()
            .filter(|running| running.context.root == context.root)
            .ok_or_else(|| failure("skill reads require the admitted physical turn"))?;
        let sources = running
            .sources
            .as_ref()
            .ok_or_else(|| failure("the turn has no captured skill context"))?;
        if sources.scope().principal != self.principal
            || sources.scope().node != self.node
            || running
                .binding
                .as_ref()
                .is_none_or(|binding| binding.path != sources.scope().directory)
        {
            return Err(failure(
                "skill source scope does not match the physical turn",
            ));
        }
        let advertised = sources.parts().iter().any(|part| {
            if part.kind() == SourceKind::SkillInstructions {
                return part.id() == args.id && part.revision() == args.revision;
            }
            if part.kind() != SourceKind::SkillCatalog {
                return false;
            }
            serde_json::from_str::<serde_json::Value>(part.content())
                .ok()
                .and_then(|catalog| {
                    catalog
                        .get("skills")
                        .and_then(|skills| skills.as_array())
                        .map(|skills| {
                            skills.iter().any(|skill| {
                                skill["id"] == args.id && skill["revision"] == args.revision
                            })
                        })
                })
                .unwrap_or(false)
        });
        if !advertised {
            return Err(failure(
                "the skill was not advertised or selected for this turn",
            ));
        }
        Ok(())
    }

    pub(super) fn validate_skill(
        &self,
        context: &Context,
        spec: &ToolSpec,
        call: &ToolCall,
    ) -> tembra::Result<ToolDecision> {
        if *spec != tool_spec()? {
            return Ok(ToolDecision::Block(
                "the skill tool definition changed".into(),
            ));
        }
        Ok(match self.authorize_skill(context, call) {
            Ok(()) => ToolDecision::Allow,
            Err(error) => ToolDecision::Block(error.message),
        })
    }

    pub(super) async fn read_skill(
        &self,
        context: &Context,
        call: &ToolCall,
    ) -> tembra::Result<ToolResult> {
        if let Err(error) = self.authorize_skill(context, call) {
            return Ok(ToolResult::Blocked(error.message));
        }
        let args: Read = serde_json::from_value(call.arguments.clone())
            .map_err(|_| failure("invalid skill read arguments"))?;
        let binding = self
            .running
            .borrow()
            .as_ref()
            .and_then(|running| running.binding.clone())
            .ok_or_else(|| failure("the physical checkout is unavailable"))?;
        checkout::validate(
            self.git.as_ref(),
            context,
            &self.node,
            &self.principal,
            &binding,
        )
        .await?;
        let configuration = self
            .environment_configuration
            .clone()
            .ok_or_else(|| failure("project content is disabled"))?;
        let selected = Selection {
            id: args.id,
            revision: args.revision,
        };
        let result = tokio::task::spawn_blocking(move || {
            open(&binding, configuration)?
                .read(&selected, &args.path)
                .map_err(source_error)
        })
        .await
        .map_err(|_| failure("skill read did not settle"))?;
        context.check()?;
        Ok(match result {
            Ok(content) => ToolResult::Success(json!({"content":content})),
            Err(error) => ToolResult::Blocked(error.message),
        })
    }
}

pub(super) struct SkillTools {
    host: Host,
    provider: ToolProvider,
    principal: String,
}

impl SkillTools {
    pub fn new(host: Host, session: String, principal: String) -> tembra::Result<Self> {
        Ok(Self {
            host,
            provider: ToolProvider::new(super::SERVICE, vec![session])?,
            principal,
        })
    }
    pub fn provider(&self) -> &ToolProvider {
        &self.provider
    }
    pub async fn call<A: 'static, R: 'static>(
        &self,
        context: &Context,
        method: &str,
        args: A,
    ) -> tembra::Result<R> {
        context.check()?;
        if context.principal()? != self.principal {
            return Err(failure("skill provider belongs to another principal"));
        }
        let reference = self.host.get(
            &self.provider.service,
            self.provider.key.clone(),
            &self.principal,
        )?;
        reference.call(context, method, args).await
    }
}
