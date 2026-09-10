use crate::{Error, NodeIdentity};
use ora_node_protocol::NodeId;
use rusqlite::Connection;

const APPLICATION_ID: i64 = 0x4f52414e;

/// Checks identity before any persistent pragma or migration touches an existing database.
pub(super) fn initialize(
    connection: &mut Connection,
    created: bool,
    identity: &NodeIdentity,
) -> Result<NodeId, Error> {
    if created {
        let id = match identity {
            NodeIdentity::Discover => NodeId::new(uuid::Uuid::new_v4().to_string()),
            NodeIdentity::Require(id) => id.clone(),
        };
        if id.as_str().trim().is_empty() {
            return Err(Error::NodeMismatch);
        }
        let tx = connection.transaction()?;
        tx.pragma_update(None, "application_id", APPLICATION_ID)?;
        tx.pragma_update(None, "user_version", 1)?;
        tx.execute_batch("CREATE TABLE node_metadata (singleton INTEGER PRIMARY KEY CHECK(singleton = 1), node_id TEXT NOT NULL CHECK(length(node_id) > 0));")?;
        tx.execute("INSERT INTO node_metadata VALUES (1, ?1)", [id.as_str()])?;
        tx.execute_batch(include_str!("schema.sql"))?;
        tx.commit()?;
    }
    let app: i64 = connection.pragma_query_value(None, "application_id", |row| row.get(0))?;
    let version: i64 = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
    if app != APPLICATION_ID || version != 1 {
        return Err(Error::InvalidSchema);
    }
    let check: String = connection.pragma_query_value(None, "integrity_check", |row| row.get(0))?;
    if check != "ok" {
        return Err(Error::InvalidSchema);
    }
    let id = NodeId::new(connection.query_row(
        "SELECT node_id FROM node_metadata WHERE singleton = 1",
        [],
        |row| row.get::<_, String>(0),
    )?);
    if id.as_str().trim().is_empty()
        || matches!(identity, NodeIdentity::Require(expected) if expected != &id)
    {
        return Err(Error::NodeMismatch);
    }
    Ok(id)
}
