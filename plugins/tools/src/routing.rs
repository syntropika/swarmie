use crate::{
    Catalog, OperationId, ToolCall, ToolDecision, ToolMethod, ToolPreparation, ToolProvider,
    ToolResult, ToolSpec, invalid,
};
use std::{
    cell::{Cell, RefCell},
    collections::BTreeMap,
    rc::Rc,
};
use tembra::{Context, Host, Reference};

/// Native claims owned by one catalog object's managed resource. This is not a
/// service registry: Tembra resolves and retains each provider under the caller's
/// principal. Claims survive preparation, dispatch and reconciliation and are all
/// released when the last catalog consumer closes its object.
pub(crate) struct Routing {
    pub catalog: Catalog,
    host: Host,
    claims: RefCell<BTreeMap<(String, ToolProvider), Rc<Reference>>>,
    closed: Cell<bool>,
}

impl Routing {
    pub fn new(host: Host, catalog: Catalog) -> Self {
        Self {
            catalog,
            host,
            claims: RefCell::new(BTreeMap::new()),
            closed: Cell::new(false),
        }
    }

    pub fn reference(
        &self,
        context: &Context,
        provider: &ToolProvider,
    ) -> tembra::Result<Rc<Reference>> {
        context.check()?;
        provider.validate()?;
        if self.closed.get() {
            return Err(invalid("catalog provider claims are closed"));
        }
        let key = (context.principal()?, provider.clone());
        if let Some(reference) = self.claims.borrow().get(&key) {
            return Ok(reference.clone());
        }
        let reference = Rc::new(
            self.host
                .get(&provider.service, provider.key.clone(), &key.0)?,
        );
        self.claims.borrow_mut().insert(key, reference.clone());
        Ok(reference)
    }

    pub fn close(&self) -> tembra::Result<()> {
        self.closed.set(true);
        let claims = std::mem::take(&mut *self.claims.borrow_mut());
        let mut failure = None;
        for reference in claims.into_values() {
            if let Err(error) = reference.release() {
                failure.get_or_insert(error);
            }
        }
        failure.map_or(Ok(()), Err)
    }

    pub async fn ready_reference(
        &self,
        context: &Context,
        provider: &ToolProvider,
    ) -> tembra::Result<Rc<Reference>> {
        let reference = self.reference(context, provider)?;
        if let Err(error) = reference.ready().await {
            let key = (context.principal()?, provider.clone());
            self.claims.borrow_mut().remove(&key);
            let _ = reference.release();
            return Err(error);
        }
        context.check()?;
        Ok(reference)
    }

    pub async fn prepare(
        &self,
        context: &Context,
        spec: ToolSpec,
        call: ToolCall,
        expected: Option<ToolProvider>,
    ) -> tembra::Result<ToolPreparation> {
        context.check()?;
        let Some(tool) = self
            .catalog
            .snapshot()?
            .tools
            .into_iter()
            .find(|tool| tool.spec == spec && tool.spec.name == call.name)
        else {
            return Ok(blocked("tool is not available with this definition"));
        };
        if expected
            .as_ref()
            .is_some_and(|expected| expected != &tool.provider)
        {
            return Ok(blocked("pending tool belongs to a different provider"));
        }
        let decision = self
            .ready_reference(context, &tool.provider)
            .await?
            .call(context, ToolMethod::Validate.as_str(), (spec, call))
            .await?;
        context.check()?;
        let still_registered = self.catalog.snapshot()?.tools.contains(&tool);
        Ok(ToolPreparation {
            decision: if still_registered {
                decision
            } else {
                ToolDecision::Block("tool was withdrawn during preparation".into())
            },
            provider: Some(tool.provider),
        })
    }

    fn admitted(&self, call: &ToolCall, target: &ToolProvider) -> bool {
        self.catalog.snapshot().is_ok_and(|selection| {
            selection
                .tools
                .iter()
                .any(|tool| tool.spec.name == call.name && &tool.provider == target)
        })
    }

    pub async fn execute(
        &self,
        context: &Context,
        operation: OperationId,
        call: ToolCall,
        target: ToolProvider,
    ) -> tembra::Result<ToolResult> {
        context.check()?;
        if !self.admitted(&call, &target) {
            return Ok(ToolResult::Blocked(
                "catalog does not currently admit this tool and provider".into(),
            ));
        }
        let reference = match self.ready_reference(context, &target).await {
            Ok(reference) => reference,
            Err(_) => {
                context.check()?;
                return Ok(ToolResult::Blocked(
                    "provider could not be activated before dispatch".into(),
                ));
            }
        };
        context.check()?;
        if !self.admitted(&call, &target) {
            return Ok(ToolResult::Blocked(
                "tool was withdrawn before dispatch".into(),
            ));
        }
        // Once the provider is called, only its result/recovery can prove outcome.
        reference
            .call(context, ToolMethod::Execute.as_str(), (operation, call))
            .await
    }
}

fn blocked(message: &str) -> ToolPreparation {
    ToolPreparation {
        decision: ToolDecision::Block(message.into()),
        provider: None,
    }
}
