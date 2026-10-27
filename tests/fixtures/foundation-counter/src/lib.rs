//! A synthetic capability with no dependency on Swarmie or PluginManager.

use tembra::{Error, Orb, Service};

pub const SERVICE: &str = "fixture.counter";

#[derive(Clone, Copy)]
pub enum Method {
    Read,
    Increment,
}

impl Method {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Read => "read",
            Self::Increment => "increment",
        }
    }
}

pub fn orb() -> Orb {
    let service = Service::new(SERVICE, |declaration| {
        declaration.state(0_u64, 1, |_| Ok(()))
    })
    .method(Method::Read.as_str(), |state, context, (): ()| async move {
        context.check()?;
        Ok(*state.value()?)
    })
    .method(
        Method::Increment.as_str(),
        |state, context, (amount,): (u64,)| async move {
            Ok(*state.update(&context, |value| {
                value
                    .checked_add(amount)
                    .ok_or_else(|| Error::invalid("fixture.counter.overflow", "counter overflow"))
            })?)
        },
    );
    Orb::new(SERVICE, 1).service(service)
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::{FutureExt, executor::LocalPool};
    use std::rc::Rc;

    #[test]
    fn capability_runs_without_swarmie_or_plugin_manager() {
        let mut executor = LocalPool::new();
        let host = tembra::Host::new(executor.spawner());
        host.install(vec![orb()]).unwrap();
        let counter = Rc::new(host.get(SERVICE, vec!["main".into()], "test").unwrap());
        let result: u64 = executor
            .run_until(host.root("test", move |context| {
                async move {
                    counter
                        .call(&context, Method::Increment.as_str(), (7_u64,))
                        .await
                }
                .boxed_local()
            }))
            .unwrap();
        assert_eq!(result, 7);
        executor.run_until(host.close()).unwrap();
        assert_eq!(host.kernel().diagnostics().objects, 0);
    }
}
