//! Durable plugin admission, atomic evidence, exact acknowledgements and runtime fencing.
use super::*;
use ora_node_protocol::*;
use pretty_assertions::assert_eq;

/// A removal needs no package server, so it isolates storage from filesystem execution.
fn command() -> PluginCommand {
    PluginCommand::Remove(RemovePluginsMessage {
        protocol_version: CURRENT_PROTOCOL_VERSION,
        operation_id: OperationId::new("plugin-op"),
        execution_id: ExecutionId::new("plugin-execution"),
        payload: RemovePlugins {
            spec: RemovePluginsSpec {
                node_id: NodeId::new("node"),
                plugins: vec![PluginRemoval {
                    plugin_id: PluginId::new("official/agent"),
                    version: PluginVersion::new("1.0.0"),
                }],
            },
        },
    })
}

/// The original incarnation is part of the retained result, even after reopening the database.
fn result() -> PluginExecutionResult {
    PluginExecutionResult::PluginsCompleted(PluginsCompleted {
        node: NodeRuntimeIdentity {
            node_id: NodeId::new("node"),
            incarnation_id: NodeIncarnationId::new("first"),
        },
        items: vec![PluginItemResult {
            plugin_id: PluginId::new("official/agent"),
            outcome: PluginItemOutcome::Removed {},
        }],
    })
}

/// Acceptance survives a restart, and a query or duplicate command cannot consume the outbox.
#[test]
fn input_result_and_exact_event_survive_restart_and_acknowledgement() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("node.sqlite3");
    let mut db = NodeDatabase::open(&path, NodeIdentity::Require(NodeId::new("node"))).unwrap();
    let owner = ControllerId::new("controller");
    db.bind_controller(&owner).unwrap();
    let command = command();
    let accepted = db.accept_plugins(&command).unwrap();
    assert_eq!(db.accept_plugins(&command).unwrap(), accepted);
    assert_eq!(
        db.unfinished_runtime_executions().unwrap(),
        vec![command.execution_id().as_str()]
    );
    drop(db);
    let mut db = NodeDatabase::open(&path, NodeIdentity::Require(NodeId::new("node"))).unwrap();
    assert_eq!(db.recoverable_plugins().unwrap(), vec![accepted]);
    assert!(
        db.start_plugins(&command, &NodeIncarnationId::new("second"))
            .unwrap()
    );
    db.complete_plugins(&command, result()).unwrap();
    let events = db.controller_events(&owner).unwrap();
    let state = ExecutionState::Completed(ExecutionResult::Plugin(result()));
    assert_eq!(
        db.execution_state(command.operation_id(), command.execution_id())
            .unwrap(),
        state
    );
    assert_eq!(db.pending_events().unwrap(), events);
    assert!(db.recoverable_plugins().unwrap().is_empty());
    drop(db);
    let mut db = NodeDatabase::open(&path, NodeIdentity::Require(NodeId::new("node"))).unwrap();
    assert_eq!(db.controller_events(&owner).unwrap(), events);
    let mut ack = EventAckMessage {
        protocol_version: CURRENT_PROTOCOL_VERSION,
        operation_id: command.operation_id().clone(),
        execution_id: command.execution_id().clone(),
        sequence: Sequence::new(/*value*/ 2),
        payload: EventAck {
            node_id: NodeId::new("node"),
        },
    };
    assert!(matches!(db.acknowledge(&ack), Err(Error::InvalidAck)));
    ack.sequence = Sequence::new(/*value*/ 1);
    db.acknowledge(&ack).unwrap();
    db.acknowledge(&ack).unwrap();
    assert!(db.controller_events(&owner).unwrap().is_empty());
    assert_eq!(db.accept_plugins(&command).unwrap().state, state);
    assert!(db.unfinished_runtime_executions().unwrap().is_empty());
}

/// Reusing an identity or supplying incomplete item evidence never changes the original input.
#[test]
fn identity_and_result_conflicts_leave_the_execution_unfinished() {
    let dir = tempfile::tempdir().unwrap();
    let mut db = NodeDatabase::open(
        &dir.path().join("node.db"),
        NodeIdentity::Require(NodeId::new("node")),
    )
    .unwrap();
    let original = command();
    db.accept_plugins(&original).unwrap();
    let PluginCommand::Remove(mut changed) = command() else {
        unreachable!()
    };
    changed.payload.spec.plugins[0].version = PluginVersion::new("2.0.0");
    assert!(matches!(
        db.accept_plugins(&PluginCommand::Remove(changed)),
        Err(Error::IdentityConflict)
    ));
    let PluginExecutionResult::PluginsCompleted(mut incomplete) = result() else {
        unreachable!()
    };
    incomplete.items.clear();
    assert!(matches!(
        db.complete_plugins(
            &original,
            PluginExecutionResult::PluginsCompleted(incomplete)
        ),
        Err(Error::IdentityConflict)
    ));
    assert_eq!(
        db.execution_state(original.operation_id(), original.execution_id())
            .unwrap(),
        ExecutionState::Accepted
    );
    assert!(db.pending_events().unwrap().is_empty());
}

/// Failing the outbox write rolls the terminal result back too, leaving recovery possible.
#[test]
fn terminal_fact_cannot_commit_without_its_event() {
    struct FailOutbox;
    impl WriteGuard for FailOutbox {
        fn before_write(&self, point: WritePoint) -> Result<(), Error> {
            if point == WritePoint::Outbox {
                Err(Error::Injected(point))
            } else {
                Ok(())
            }
        }
    }
    let dir = tempfile::tempdir().unwrap();
    let mut db = NodeDatabase::open_with_guard(
        &dir.path().join("node.db"),
        NodeIdentity::Require(NodeId::new("node")),
        FailOutbox,
    )
    .unwrap();
    let command = command();
    db.accept_plugins(&command).unwrap();
    assert!(matches!(
        db.complete_plugins(&command, result()),
        Err(Error::Injected(WritePoint::Outbox))
    ));
    assert_eq!(
        db.execution_state(command.operation_id(), command.execution_id())
            .unwrap(),
        ExecutionState::Accepted
    );
    assert!(db.pending_events().unwrap().is_empty());
}

/// Enabling Cloud control is durable and cannot be bypassed by the local plugin entry point.
#[test]
fn runtime_enforcement_refuses_bare_plugin_commands() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("node.db");
    let mut db = NodeDatabase::open(&path, NodeIdentity::Require(NodeId::new("node"))).unwrap();
    db.enforce_runtime_control().unwrap();
    drop(db);
    let mut db = NodeDatabase::open(&path, NodeIdentity::Require(NodeId::new("node"))).unwrap();
    assert!(matches!(
        db.accept_plugins(&command()),
        Err(Error::InvalidTransition)
    ));
    assert!(db.recoverable_plugins().unwrap().is_empty());
}

/// A Cloud permit cannot survive input closure or be reused by a restarted Node incarnation.
#[test]
fn controlled_plugin_start_requires_current_open_incarnation() {
    for close in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("node.db");
        let mut db = NodeDatabase::open(&path, NodeIdentity::Require(NodeId::new("node"))).unwrap();
        let mut scope = super::repository::runtime_scope(db.node_id());
        scope.operation_id = "business-operation".into();
        db.bind_runtime(&scope).unwrap();
        let mut permit = scope.clone();
        permit.execution_id = command().execution_id().as_str().into();
        permit.node_operation_id = command().operation_id().as_str().into();
        let input = ControlledPlugins {
            binding: permit,
            command: command(),
        };
        db.accept_controlled_plugins(&input).unwrap();
        assert!(
            db.start_plugins(&input.command, &NodeIncarnationId::new("host-incarnation"))
                .unwrap()
        );
        drop(db);
        let mut db = NodeDatabase::open(&path, NodeIdentity::Require(NodeId::new("node"))).unwrap();
        if close {
            scope.input_closed = true;
            scope.control_version += 1;
            assert_eq!(
                db.bind_runtime(&scope).unwrap(),
                vec![input.command.execution_id().as_str()]
            );
        } else {
            db.enforce_runtime_incarnation("replacement").unwrap();
        }
        assert!(
            !db.start_plugins(&input.command, &NodeIncarnationId::new("replacement"))
                .unwrap()
        );
        assert!(db.accept_controlled_plugins(&input).is_err());
        assert_eq!(db.recoverable_plugins().unwrap().len(), 1);
    }
}
