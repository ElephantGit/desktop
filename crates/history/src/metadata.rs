use agent_client_protocol_schema::v1::Meta;

/// Identifies a valid OpenCode projection; cleared or malformed sources belong to the parent.
pub(super) fn child_source_id(meta: Option<&Meta>) -> Option<&str> {
    let source = meta?.get("opencode/child-session")?;
    let id = source.get("id")?.as_str().filter(|id| !id.is_empty())?;
    source
        .get("parentID")?
        .as_str()
        .filter(|parent| !parent.is_empty())?;
    source.get("depth")?.as_u64().filter(|depth| *depth > 0)?;
    Some(id)
}

/// Retains reported tool context while assembling one durable snapshot.
///
/// Metadata namespaces are opaque replacement values, not a recursive patch protocol.
/// Omitted namespaces survive; explicit null remains authoritative.
pub(super) fn retain_tool_metadata(target: &mut Option<Meta>, incoming: Option<&Meta>) {
    let Some(incoming) = incoming else {
        return;
    };
    let target = target.get_or_insert_with(Meta::new);
    for (key, value) in incoming {
        target.insert(key.clone(), value.clone());
    }
}
