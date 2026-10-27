//! A domain orb independent of Durable Tasks, PluginManager and storage backends.
use futures::future::LocalBoxFuture;
use std::rc::Rc;
use tembra::{Context, Orb, Service};

pub const SERVICE: &str = "fixture.journal";

#[derive(Clone, Copy)]
pub enum Method {
    Append,
}

impl Method {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Append => "append",
        }
    }
}

/// Success acknowledges staging. The encompassing commit confirms durability.
pub trait Journal {
    fn append(&self, context: Context, entry: String) -> LocalBoxFuture<'_, tembra::Result<()>>;
}

pub fn orb(journal: Rc<dyn Journal>) -> Orb {
    Orb::new(SERVICE, 1).service(Service::new(SERVICE, move |_| Ok(journal.clone())).method(
        Method::Append.as_str(),
        |journal, context, (entry,): (String,)| async move { journal.append(context, entry).await },
    ))
}
