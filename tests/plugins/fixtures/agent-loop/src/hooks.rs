//! Lifecycle-owned hook contributors installed through ordinary native Facets.

use std::{collections::BTreeMap, rc::Rc};

use futures::future::LocalBoxFuture;
use tembra::{NativeFacet, Result, contracts::Contract, core};
use tembra_hooks::{HookArgs, RustHookDefinition, hook_handler};

/// Builds one actual Registry contribution without introducing a hook dispatcher.
/// The owning Facet controls installation, admission, retirement and operation settlement.
pub fn contribution<A: 'static, R: 'static, H: 'static>(
    definition: RustHookDefinition<A, R>,
    key: &str,
    priority: i64,
    handler: impl Fn(HookArgs<A, H>, core::Context) -> LocalBoxFuture<'static, Result<H>> + 'static,
) -> Result<NativeFacet> {
    let contract = Rc::new(
        Contract::local(
            format!("fixture.agent-hook.{key}"),
            1,
            serde_json::json!({}),
            BTreeMap::from([("run".into(), definition.descriptor().contribution_member())]),
        )
        .map_err(|error| {
            tembra::RuntimeError::invalid("fixture_hook_contract", error.to_string())
        })?,
    );
    let key = key.to_owned();
    let handler = hook_handler(Rc::new(handler));
    NativeFacet::new(
        format!("fixture.agent-hook.{key}"),
        vec![],
        move |_, setup| {
            let definition = definition.clone();
            let key = key.clone();
            let handler = handler.clone();
            setup.core_mut().serve(
                contract.clone(),
                Rc::new(move |_, _, declaration| {
                    definition.register(declaration, key.clone(), priority, "run")?;
                    let handler = handler.clone();
                    Ok(core::Implementation::new()
                        .method("run", move |args, context| handler(args, context)))
                }),
                core::ServeOptions {
                    lifetime: core::Lifetime::Host,
                    ..Default::default()
                },
            )
        },
    )
}
