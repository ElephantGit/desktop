use ora_controller_proto::v1::PluginInstall;
use pretty_assertions::assert_eq;
use prost::Message;

/// Cloud compares the re-encoded dispatch with its frozen input, including an empty association.
#[test]
fn frozen_plugin_declaration_survives_the_controller_wire_round_trip()
-> Result<(), Box<dyn std::error::Error>> {
    // Cloud's PluginInstall field 5 is an optional PluginAgentConfiguration message.
    // These fixed wire fixtures remain independent of this client's generated schema, so an
    // older pin that silently drops the association cannot make both sides of the assertion pass.
    for association in [vec![], vec![0x08, 1, 0x12, 3, b'a', b'b', b'c']] {
        let mut wire = vec![0x0a, 1, b'p', 0x12, 1, b'v', 0x2a];
        wire.push(association.len().try_into()?);
        wire.extend(association);
        let decoded = PluginInstall::decode(wire.as_slice())?;
        assert_eq!(decoded.encode_to_vec(), wire);
    }
    Ok(())
}
