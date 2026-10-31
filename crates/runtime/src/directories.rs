//! Optional directory navigation supplied by a node embedding.

use futures::future::LocalBoxFuture;
use serde::{Deserialize, Serialize};
use std::rc::Rc;
use tembra::{Code, Error, Orb, Service};

/// Read-only folder navigation, independent of project links and file access grants.
pub const DIRECTORIES_SERVICE: &str = "swarmie.directories";

/// A null path opens the embedding's permitted root. Cursors belong to that directory.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DirectoryRequest {
    pub path: Option<String>,
    pub cursor: Option<String>,
}

/// A directory path reported by the node, not reconstructed by its clients.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DirectoryEntry {
    pub name: String,
    pub path: String,
}

/// One bounded page. The embedding controls root boundaries and path authorization.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DirectoryPage {
    pub path: String,
    pub parent: Option<String>,
    pub directories: Vec<DirectoryEntry>,
    pub next: Option<String>,
}

/// The embedding supplies I/O and checks the authenticated principal on each read.
/// Listing directories does not grant file access, write access or project membership.
pub trait DirectorySource {
    fn list(
        &self,
        principal: String,
        request: DirectoryRequest,
    ) -> LocalBoxFuture<'static, tembra::Result<DirectoryPage>>;
}

/// Compose an optional, managed service without adding I/O to the bare runtime.
pub fn directories_orb(source: Rc<dyn DirectorySource>) -> Orb {
    Orb::new(DIRECTORIES_SERVICE, 1).service(
        Service::new(DIRECTORIES_SERVICE, move |declaration| {
            if declaration.address()?.key != ["main"] {
                return Err(Error::new(
                    Code::Forbidden,
                    "directories.key",
                    "use the main key",
                ));
            }
            Ok(source.clone())
        })
        .method(
            "list",
            |source, context, (request,): (DirectoryRequest,)| async move {
                context.check()?;
                let result = source
                    .list(context.principal()?.to_owned(), request)
                    .await?;
                context.check()?;
                Ok(result)
            },
        ),
    )
}
