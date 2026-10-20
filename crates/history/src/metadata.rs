use agent_client_protocol_schema::v1::Meta;

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
