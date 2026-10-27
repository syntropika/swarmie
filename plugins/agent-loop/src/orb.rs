use crate::{Agent, Request, State};
use tembra::{Declaration, Orb, Service};

pub const SERVICE: &str = "swarmie.agent-loop";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Method {
    Execute,
    Resume,
}

impl Method {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Execute => "execute",
            Self::Resume => "resume",
        }
    }
}

/// Ordinary managed invocations. No task scheduler, history store or manager is required.
pub fn orb(create: impl Fn(Declaration) -> tembra::Result<Agent> + 'static) -> Orb {
    crate::hooks::Hooks::default().declare(Orb::new(SERVICE, 1)).service(Service::new(SERVICE, move |declaration| {
        create(declaration.clone())?.bind_hooks(&declaration)
    }).method(
        Method::Execute.as_str(),
        |agent, context, request: Request| async move { Ok(agent.execute(&context, request).await) },
    ).method(Method::Resume.as_str(), |agent, context, state: State| async move {
        Ok(agent.resume(&context, state).await)
    }))
}
