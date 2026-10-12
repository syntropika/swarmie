use super::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum NodeTransport {
    Http {
        url: String,
    },
    Websocket {
        url: String,
    },
    LocalSocket {
        path: String,
    },
    Ssh {
        host: String,
        #[serde(rename = "socketPath")]
        socket_path: String,
    },
}
impl NodeTransport {
    fn validate(&self) -> Result<()> {
        match self {
            Self::Http { url } | Self::Websocket { url } => {
                let parsed = url::Url::parse(url)
                    .map_err(|_| Error::Invalid("invalid node endpoint".into()))?;
                let correct_scheme = match self {
                    Self::Http { .. } => matches!(parsed.scheme(), "http" | "https"),
                    _ => matches!(parsed.scheme(), "ws" | "wss"),
                };
                if !correct_scheme
                    || parsed.host_str().is_none()
                    || !parsed.username().is_empty()
                    || parsed.password().is_some()
                    || parsed.query().is_some()
                    || parsed.fragment().is_some()
                    || url.len() > 8192
                {
                    return Err(Error::Invalid("node endpoint requires a supported URL without credentials, query, or fragment".into()));
                }
            }
            Self::LocalSocket { path } => validate_socket_path(path)?,
            Self::Ssh { host, socket_path } => {
                if host.is_empty()
                    || host.starts_with('-')
                    || host.chars().any(|c| c.is_whitespace() || c.is_control())
                {
                    return Err(Error::Invalid("invalid SSH host".into()));
                }
                if !socket_path.is_empty() { validate_socket_path(socket_path)?; }
            }
        }
        Ok(())
    }
    pub fn endpoint(&self) -> String {
        match self {
            Self::Http { url } | Self::Websocket { url } => url.clone(),
            Self::LocalSocket { path } => {
                let mut url = url::Url::parse("unix:///").expect("static URL");
                url.set_path(path);
                url.to_string()
            }
            Self::Ssh { host, socket_path } => {
                let mut url = url::Url::parse("ssh://runtime/").expect("static URL");
                url.query_pairs_mut()
                    .append_pair("host", host)
                    .append_pair("socket", socket_path);
                url.to_string()
            }
        }
    }
}
fn validate_socket_path(path: &str) -> Result<()> {
    if !std::path::Path::new(path).is_absolute()
        || path.len() > 8192
        || path.chars().any(char::is_control)
    {
        return Err(Error::Invalid(
            "socket path must be absolute and contain no control characters".into(),
        ));
    }
    Ok(())
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NodeConnection {
    pub name: String,
    pub identity: LocalStoreIdentity,
    pub transport: NodeTransport,
    pub credential_ref: Option<String>,
    pub revision: u64,
    pub created_at_ms: u64,
    pub last_verified_at_ms: u64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeRecord {
    pub identity: LocalStoreIdentity,
    pub display_name: String,
    pub local: bool,
    pub registered: bool,
    pub created_at_ms: u64,
    pub last_verified_at_ms: u64,
    pub connections: Vec<NodeConnection>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeCatalog {
    pub selected_node_id: Option<String>,
    pub nodes: Vec<NodeRecord>,
}
fn name_valid(name: &str) -> Result<()> {
    if name.trim().is_empty() || name.len() > 100 || name.chars().any(char::is_control) {
        return Err(Error::Invalid(
            "connection name must contain 1 to 100 printable characters".into(),
        ));
    }
    Ok(())
}
fn identity_valid(identity: &LocalStoreIdentity) -> Result<()> {
    for id in [
        &identity.environment_id,
        &identity.store_id,
        &identity.owner_scope_id,
    ] {
        validate_id(id)?;
    }
    Ok(())
}
fn unsigned(row: &rusqlite::Row<'_>, index: usize) -> rusqlite::Result<u64> {
    let value: i64 = row.get(index)?;
    u64::try_from(value).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(
            index,
            rusqlite::types::Type::Integer,
            Box::new(error),
        )
    })
}
fn connections(connection: &Connection) -> Result<Vec<NodeConnection>> {
    let mut query = connection.prepare("SELECT c.name,n.environment_id,n.store_id,n.owner_scope_id,c.transport_json,c.credential_ref,c.revision,c.created_at_ms,c.last_verified_at_ms FROM node_connections c JOIN node_records n USING(environment_id) ORDER BY c.name").map_err(database_error)?;
    let rows = query
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                LocalStoreIdentity {
                    environment_id: row.get(1)?,
                    store_id: row.get(2)?,
                    owner_scope_id: row.get(3)?,
                },
                row.get::<_, String>(4)?,
                row.get::<_, Option<String>>(5)?,
                unsigned(row, 6)?,
                unsigned(row, 7)?,
                unsigned(row, 8)?,
            ))
        })
        .map_err(database_error)?;
    rows.map(|row| {
        let (
            name,
            identity,
            transport,
            credential_ref,
            revision,
            created_at_ms,
            last_verified_at_ms,
        ) = row.map_err(database_error)?;
        Ok(NodeConnection {
            name,
            identity,
            transport: serde_json::from_str(&transport)?,
            credential_ref,
            revision,
            created_at_ms,
            last_verified_at_ms,
        })
    })
    .collect()
}
fn upsert_node(
    connection: &Connection,
    identity: &LocalStoreIdentity,
    name: &str,
    local: bool,
    now: u64,
) -> Result<()> {
    let retained: Option<(String, String)> = connection
        .query_row(
            "SELECT store_id,owner_scope_id FROM node_records WHERE environment_id=?1",
            [&identity.environment_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(database_error)?;
    if retained.is_some_and(|(store, owner)| {
        store != identity.store_id || owner != identity.owner_scope_id
    }) {
        return Err(Error::Invalid(
            "environment identity changed; enroll a different node deliberately".into(),
        ));
    }
    connection.execute("INSERT INTO node_records VALUES (?1,?2,?3,?4,?5,?6,?6) ON CONFLICT(environment_id) DO UPDATE SET local=max(local,excluded.local),display_name=CASE WHEN excluded.local=1 THEN excluded.display_name ELSE display_name END,last_verified_at_ms=max(last_verified_at_ms,excluded.last_verified_at_ms)",
        params![identity.environment_id,identity.store_id,identity.owner_scope_id,name,local,integer(now)?]).map_err(database_error)?;
    Ok(())
}
impl SqliteSessionStore {
    /// Record verified automatic local attachment; this does not connect or start execution.
    pub async fn register_local_node(&self, verified: LocalStoreIdentity, now: u64) -> Result<()> {
        if verified != self.identity {
            return Err(Error::Invalid(
                "local node differs from the configured store".into(),
            ));
        }
        self.database(move |connection| {
            let transaction = connection
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .map_err(database_error)?;
            upsert_node(&transaction, &verified, "This computer", true, now)?;
            transaction
                .execute(
                    "INSERT INTO selected_node VALUES (1,?1) ON CONFLICT DO NOTHING",
                    [&verified.environment_id],
                )
                .map_err(database_error)?;
            transaction.commit().map_err(database_error)
        })
        .await
    }
    pub async fn node_connections(&self) -> Result<Vec<NodeConnection>> {
        self.database(|connection| connections(connection)).await
    }
    /// Atomically install a verified route. Revision zero means no route existed at verification start.
    pub async fn register_node_connection(
        &self,
        mut incoming: NodeConnection,
        expected_revision: u64,
    ) -> Result<NodeConnection> {
        name_valid(&incoming.name)?;
        identity_valid(&incoming.identity)?;
        if let Some(reference) = &incoming.credential_ref {
            validate_id(reference)?;
        }
        incoming.transport.validate()?;
        self.database(move |connection| {
            let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate).map_err(database_error)?;
            let current = connections(&transaction)?.into_iter().find(|c| c.name == incoming.name);
            if current.as_ref().is_some_and(|c| c.identity != incoming.identity) {
                return Err(Error::Invalid("environment identity changed; remove the saved connection before pairing a different environment".into()));
            }
            if current.as_ref().map_or(0,|c| c.revision) != expected_revision {
                return Err(Error::Invalid("node connection changed during verification; retry against current settings".into()));
            }
            transaction.execute("UPDATE node_registry_clock SET revision=revision+1 WHERE singleton=1", []).map_err(database_error)?;
            incoming.revision = transaction.query_row("SELECT revision FROM node_registry_clock WHERE singleton=1", [], |r| unsigned(r,0)).map_err(database_error)?;
            incoming.created_at_ms = current.as_ref().map_or(incoming.last_verified_at_ms, |c| c.created_at_ms);
            incoming.last_verified_at_ms = current.as_ref().map_or(incoming.last_verified_at_ms,|c|c.last_verified_at_ms.max(incoming.last_verified_at_ms));
            upsert_node(&transaction, &incoming.identity, &incoming.name, false, incoming.last_verified_at_ms)?;
            transaction.execute("INSERT INTO node_connections VALUES (?1,?2,?3,?4,?5,?6,?7) ON CONFLICT(name) DO UPDATE SET transport_json=excluded.transport_json,credential_ref=excluded.credential_ref,revision=excluded.revision,last_verified_at_ms=excluded.last_verified_at_ms",params![incoming.name,incoming.identity.environment_id,serde_json::to_string(&incoming.transport)?,incoming.credential_ref,integer(incoming.revision)?,integer(incoming.created_at_ms)?,integer(incoming.last_verified_at_ms)?]).map_err(database_error)?;
            transaction.commit().map_err(database_error)?;
            Ok(incoming)
        }).await
    }
    /// Remove only client connection metadata. Node identity is retained for caches and history.
    pub async fn remove_node_connection(
        &self,
        name: String,
        expected_revision: u64,
    ) -> Result<NodeConnection> {
        self.database(move |connection| {
            let transaction = connection
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .map_err(database_error)?;
            let route = connections(&transaction)?
                .into_iter()
                .find(|c| c.name == name)
                .ok_or_else(|| Error::Invalid("unknown connection".into()))?;
            if route.revision != expected_revision {
                return Err(Error::Invalid(
                    "node connection changed; retry removal".into(),
                ));
            }
            transaction
                .execute("DELETE FROM node_connections WHERE name=?1", [name])
                .map_err(database_error)?;
            transaction.commit().map_err(database_error)?;
            Ok(route)
        })
        .await
    }
    /// Refresh historical observation only if the verified route is still current.
    pub async fn observe_node_connection(
        &self,
        name: String,
        expected_revision: u64,
        now: u64,
    ) -> Result<()> {
        self.database(move |connection| {
            let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate).map_err(database_error)?;
            let changed = transaction.execute("UPDATE node_connections SET last_verified_at_ms=max(last_verified_at_ms,?1) WHERE name=?2 AND revision=?3", params![integer(now)?,name,integer(expected_revision)?]).map_err(database_error)?;
            if changed != 1 { return Err(Error::Invalid("node connection changed during verification; reconnect".into())); }
            transaction.execute("UPDATE node_records SET last_verified_at_ms=max(last_verified_at_ms,?1) WHERE environment_id=(SELECT environment_id FROM node_connections WHERE name=?2)",params![integer(now)?,name]).map_err(database_error)?;
            transaction.commit().map_err(database_error)
        }).await
    }
    pub async fn node_catalog(&self) -> Result<NodeCatalog> {
        self.database(|connection| {
            let transaction = connection.transaction().map_err(database_error)?;
            let routes = connections(&transaction)?;
            let selected_node_id = transaction.query_row("SELECT environment_id FROM selected_node WHERE singleton=1", [], |r| r.get(0)).optional().map_err(database_error)?;
            let mut query = transaction.prepare("SELECT environment_id,store_id,owner_scope_id,display_name,local,created_at_ms,last_verified_at_ms FROM node_records ORDER BY local DESC,display_name,environment_id").map_err(database_error)?;
            let nodes = query.query_map([], |row| Ok(NodeRecord {
                identity: LocalStoreIdentity { environment_id: row.get(0)?, store_id: row.get(1)?, owner_scope_id: row.get(2)? },
                display_name: row.get(3)?, local: row.get(4)?, registered: false,
                created_at_ms: unsigned(row, 5)?, last_verified_at_ms: unsigned(row, 6)?, connections: vec![],
            })).map_err(database_error)?.map(|row| {
                let mut node = row.map_err(database_error)?;
                node.connections = routes.iter().filter(|r| r.identity == node.identity).cloned().collect();
                node.registered = node.local || !node.connections.is_empty();
                Ok(node)
            }).collect::<Result<Vec<_>>>()?;
            Ok(NodeCatalog { selected_node_id, nodes })
        }).await
    }
    pub async fn select_node(&self, environment_id: String) -> Result<()> {
        self.database(move |connection| {
            let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate).map_err(database_error)?;
            let registered: bool = transaction.query_row("SELECT local=1 OR EXISTS(SELECT 1 FROM node_connections c WHERE c.environment_id=n.environment_id) FROM node_records n WHERE environment_id=?1", [&environment_id], |r| r.get(0)).optional().map_err(database_error)?.unwrap_or(false);
            if !registered { return Err(Error::Invalid("node is not registered".into())); }
            transaction.execute("INSERT INTO selected_node VALUES (1,?1) ON CONFLICT(singleton) DO UPDATE SET environment_id=excluded.environment_id", [environment_id]).map_err(database_error)?;
            transaction.commit().map_err(database_error)
        }).await
    }
}
