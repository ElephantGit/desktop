use super::*;
use pretty_assertions::assert_eq;

/// Queries the production boundary and checks both the projection and durable read-only behavior.
async fn assert_projection(
    fixture: &Fixture,
    state: BindingHistoryState,
    segments: Vec<SessionBindingSegment>,
) {
    let path = ora_history::history_path(&fixture._temporary.path().join("sessions"), "session-1")
        .unwrap();
    let before_history = std::fs::read(&path).unwrap();
    let before_rows = fixture.sessions.repository.list_sessions().unwrap();
    assert_eq!(
        fixture
            .sessions
            .bindings(GetSessionBindingsRequest {
                session_ids: vec!["session-1".into()],
            })
            .await
            .unwrap(),
        GetSessionBindingsResponse {
            sessions: vec![SessionBindings::Available {
                session_id: "session-1".into(),
                history: state,
                segments,
            }],
        }
    );
    assert_eq!(std::fs::read(path).unwrap(), before_history);
    assert_eq!(
        fixture.sessions.repository.list_sessions().unwrap(),
        before_rows
    );
}

/// Gaps and ambiguous sequence entries cannot promote an earlier matching binding to current.
#[test]
fn gaps_and_conflicting_sequences_preserve_only_verified_prefixes() {
    run_test(async {
        for conflicting_sequence in [false, true] {
            let fixture = Fixture::stopped();
            let mut segments = history(
                &fixture,
                vec![HistoryRecord::Meta(meta(&fixture, "provider-session"))],
            );
            let path =
                ora_history::history_path(&fixture._temporary.path().join("sessions"), "session-1")
                    .unwrap();
            let mut bytes = std::fs::read(&path).unwrap();
            let records = if conflicting_sequence {
                vec![
                    (
                        1,
                        HistoryRecord::AgentSwitched(AgentSwitch {
                            from: AgentRef::parse("ora-space.fixture").unwrap(),
                            to: AgentRef::parse("ora-space.other").unwrap(),
                            agent_session_id: "ambiguous-one".into(),
                        }),
                    ),
                    (
                        1,
                        HistoryRecord::AgentSwitched(AgentSwitch {
                            from: AgentRef::parse("ora-space.fixture").unwrap(),
                            to: AgentRef::parse("ora-space.other").unwrap(),
                            agent_session_id: "ambiguous-two".into(),
                        }),
                    ),
                    (
                        2,
                        HistoryRecord::AgentSwitched(AgentSwitch {
                            from: AgentRef::parse("ora-space.other").unwrap(),
                            to: AgentRef::parse("ora-space.fixture").unwrap(),
                            agent_session_id: "provider-session".into(),
                        }),
                    ),
                ]
            } else {
                vec![(
                    1,
                    HistoryRecord::Gap {
                        reason: "sensitive-write-failure".into(),
                    },
                )]
            };
            for (seq, record) in records {
                bytes.extend(
                    serde_json::to_vec(&HistoryLine::new("2026-10-22T10:01:00+08:00", seq, record))
                        .unwrap(),
                );
                bytes.push(b'\n');
            }
            std::fs::write(path, bytes).unwrap();
            segments.push(SessionBindingSegment {
                key: "live".into(),
                agent_ref: "ora-space.fixture".into(),
                agent_session_id: "provider-session".into(),
                recorded_at: None,
                current: true,
            });
            assert_projection(&fixture, BindingHistoryState::Incomplete, segments).await;
        }
    });
}

/// Wall-clock and append order must not override the sequence assigned by the history owner.
#[test]
fn binding_order_follows_sequence_despite_reversed_timestamps_and_append_order() {
    run_test(async {
        let fixture = Fixture::stopped();
        let mut segments = history(
            &fixture,
            vec![
                HistoryRecord::Meta(meta(&fixture, "old-session")),
                HistoryRecord::AgentSwitched(AgentSwitch {
                    from: AgentRef::parse("ora-space.fixture").unwrap(),
                    to: AgentRef::parse("ora-space.other").unwrap(),
                    agent_session_id: "middle-session".into(),
                }),
                HistoryRecord::AgentSwitched(AgentSwitch {
                    from: AgentRef::parse("ora-space.other").unwrap(),
                    to: AgentRef::parse("ora-space.fixture").unwrap(),
                    agent_session_id: "provider-session".into(),
                }),
            ],
        );
        let path =
            ora_history::history_path(&fixture._temporary.path().join("sessions"), "session-1")
                .unwrap();
        let mut lines: Vec<HistoryLine> = std::fs::read_to_string(&path)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        for (index, line) in lines.iter_mut().enumerate() {
            line.at = format!("2026-10-22T10:00:0{}+08:00", 2 - index);
            segments[index].recorded_at = Some(line.at.clone());
        }
        lines.reverse();
        let bytes = lines
            .iter()
            .map(|line| serde_json::to_string(line).unwrap() + "\n")
            .collect::<String>();
        std::fs::write(path, bytes).unwrap();
        segments.last_mut().unwrap().current = true;
        assert_projection(&fixture, BindingHistoryState::Readable, segments).await;
    });
}

/// Retained history cannot invent a current binding or resurrect a soft-deleted session.
#[test]
fn historical_bindings_survive_unbinding_but_never_resurrect_deleted_sessions() {
    run_test(async {
        let fixture = Fixture::stopped();
        let segments = history(
            &fixture,
            vec![HistoryRecord::Meta(meta(&fixture, "old-session"))],
        );
        fixture
            .sessions
            .repository
            .update_session_binding(
                &SessionId::new("session-1"),
                AgentRef::parse("ora-space.fixture").unwrap(),
                "",
                /*now*/ 2,
            )
            .unwrap();
        assert_projection(&fixture, BindingHistoryState::Readable, segments).await;
        fixture
            .sessions
            .repository
            .soft_delete_session(&SessionId::new("session-1"), /*deleted_at*/ 3)
            .unwrap();
        let path =
            ora_history::history_path(&fixture._temporary.path().join("sessions"), "session-1")
                .unwrap();
        let before_history = std::fs::read(&path).unwrap();
        assert_eq!(
            fixture
                .sessions
                .bindings(GetSessionBindingsRequest {
                    session_ids: vec!["session-1".into()],
                })
                .await
                .unwrap(),
            GetSessionBindingsResponse {
                sessions: vec![SessionBindings::NotFound {
                    session_id: "session-1".into()
                }],
            }
        );
        assert_eq!(std::fs::read(path).unwrap(), before_history);
        assert_eq!(
            fixture
                .sessions
                .repository
                .find_session(&SessionId::new("session-1"))
                .unwrap(),
            None
        );
    });
}
