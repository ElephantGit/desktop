use agent_client_protocol_schema::v1::Meta;

/// Identifies a valid OpenCode projection; cleared or malformed sources belong to the parent.
///
/// Transcript attribution and prompt liveness share this one rule, so a source history calls
/// the parent's can never hold a different ownership in the inactivity timer.
pub fn child_source_id(meta: Option<&Meta>) -> Option<&str> {
    let source = meta?.get("opencode/child-session")?;
    let id = source
        .get("id")?
        .as_str()
        .filter(|id| !id.trim().is_empty())?;
    source
        .get("parentID")?
        .as_str()
        .filter(|parent| !parent.trim().is_empty())?;
    // JavaScript parses 1 and 1.0 as the same safe integer; replay must accept both.
    let depth = source.get("depth")?.as_f64()?;
    if !depth.is_finite() || depth < 1.0 || depth.fract() != 0.0 || depth > 9_007_199_254_740_991.0
    {
        return None;
    }
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
