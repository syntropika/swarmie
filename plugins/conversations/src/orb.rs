use crate::storage::Reason;
use crate::{ConversationId, Conversations, EntryDraft, Head, HistoryPoint, Read, StorageFactory};
use futures::FutureExt;
use tembra::{Orb, Service};

pub const SERVICE: &str = "swarmie.conversations";
pub const INSTANCE_KEY: &str = "main";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Method {
    Create,
    Get,
    Read,
    Append,
    Fork,
}

impl Method {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Create => "create",
            Self::Get => "get",
            Self::Read => "read",
            Self::Append => "append",
            Self::Fork => "fork",
        }
    }
}

/// The host owns the service lease; each method authorizes the invoking principal.
/// Tembra opens and closes storage with the service object's actual lifecycle.
pub fn orb(storage: StorageFactory) -> Orb {
    let service = Service::new(SERVICE, move |declaration| {
        let address = declaration.address()?;
        if address.principal != "host" || address.key != [INSTANCE_KEY] {
            return Err(tembra::Error::new(
                tembra::Code::Forbidden,
                Reason::Instance.as_str(),
                "conversations has one host instance",
            ));
        }
        let factory = storage.clone();
        declaration.resource_async(
            "conversations.storage",
            1,
            &[],
            move |_| async move { Ok(Conversations::new(factory().await?)) }.boxed_local(),
            |conversations, _| async move { conversations.close().await.map_err(Into::into) }.boxed_local(),
        )
    })
    .lifetime(tembra::core::Lifetime::Host, 0)
    .method(Method::Create.as_str(), |env, context, (id,): (ConversationId,)| async move {
        env.value()?.create(&context, id).await
    })
    .method(Method::Get.as_str(), |env, context, (id,): (ConversationId,)| async move {
        env.value()?.get(&context, id).await
    })
    .method(Method::Read.as_str(), |env, context, request: Read| async move {
        env.value()?.read(&context, request).await
    })
    .method(Method::Append.as_str(), |env, context, (at, entries): (Head, Vec<EntryDraft>)| async move {
        env.value()?.append(&context, at, entries).await
    })
    .method(Method::Fork.as_str(), |env, context, (id, origin): (ConversationId, HistoryPoint)| async move {
        env.value()?.fork(&context, id, origin).await
    });
    Orb::new(SERVICE, 1).service(service)
}
