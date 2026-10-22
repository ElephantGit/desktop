use super::*;
use ora_history::{AgentSwitch, HistoryLine, HistoryRecord, SCHEMA_VERSION, SessionMeta};
use pretty_assertions::assert_eq;

/// Writes real JSONL beside the real SQLite fixture; no actor or plugin is installed.
fn history(fixture: &Fixture, records: Vec<HistoryRecord>) -> Vec<SessionBindingSegment> {
    let root = fixture._temporary.path().join("sessions");
    std::fs::create_dir_all(&root).unwrap();
    let mut bytes = String::new();
    let mut expected = Vec::new();
    for (seq, record) in records.into_iter().enumerate() {
        let binding = match &record {
            HistoryRecord::Meta(meta) => Some((&meta.agent_ref, &meta.agent_session_id)),
            HistoryRecord::AgentSwitched(switch) => Some((&switch.to, &switch.agent_session_id)),
            _ => None,
        };
        if let Some((agent, id)) = binding {
            expected.push(SessionBindingSegment {
                key: format!("history:{seq}"),
                agent_ref: agent.to_string(),
                agent_session_id: id.clone(),
                recorded_at: Some("2026-10-22T10:00:00+08:00".into()),
                current: false,
            });
        }
        let line = HistoryLine::new("2026-10-22T10:00:00+08:00", seq as u32, record);
        bytes.push_str(&serde_json::to_string(&line).unwrap());
        bytes.push('\n');
    }
    let path = ora_history::history_path(&root, "session-1").unwrap();
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, bytes).unwrap();
    expected
}

/// Gets the authoritative workspace identity through the public session API.
fn meta(fixture: &Fixture, provider_id: &str) -> SessionMeta {
    let session = fixture
        .sessions
        .get(GetSessionRequest {
            session_id: "session-1".into(),
        })
        .unwrap()
        .session;
    SessionMeta {
        schema_version: SCHEMA_VERSION,
        session_id: session.id,
        workspace_id: session.workspace_id,
        agent_ref: AgentRef::parse("ora-space.fixture").unwrap(),
        agent_session_id: provider_id.into(),
        cwd: fixture._temporary.path().into(),
    }
}

#[test]
fn bindings_preserve_repeated_segments_and_ignore_handoff_without_starting_plugins() {
    run_test(async {
        let fixture = Fixture::stopped();
        let mut expected = history(
            &fixture,
            vec![
                HistoryRecord::Meta(meta(&fixture, "provider-session")),
                HistoryRecord::AgentSwitched(AgentSwitch {
                    from: AgentRef::parse("ora-space.fixture").unwrap(),
                    to: AgentRef::parse("ora-space.other").unwrap(),
                    agent_session_id: "other-session".into(),
                }),
                HistoryRecord::HandoffDelivered {
                    agent_session_id: "other-session".into(),
                },
                HistoryRecord::AgentSwitched(AgentSwitch {
                    from: AgentRef::parse("ora-space.other").unwrap(),
                    to: AgentRef::parse("ora-space.fixture").unwrap(),
                    agent_session_id: "provider-session".into(),
                }),
            ],
        );
        expected.last_mut().unwrap().current = true;
        let actual = fixture
            .sessions
            .bindings(GetSessionBindingsRequest {
                session_ids: vec!["session-1".into(), "missing".into(), "session-1".into()],
            })
            .await
            .unwrap();
        assert_eq!(
            actual,
            GetSessionBindingsResponse {
                sessions: vec![
                    SessionBindings::NotFound {
                        session_id: "missing".into()
                    },
                    SessionBindings::Available {
                        session_id: "session-1".into(),
                        history: BindingHistoryState::Readable,
                        segments: expected
                    }
                ]
            }
        );
        let public = serde_json::to_value(
            fixture
                .sessions
                .get(GetSessionRequest {
                    session_id: "session-1".into(),
                })
                .unwrap(),
        )
        .unwrap();
        assert!(!public.to_string().contains("provider-session"));
    });
}

#[test]
fn bindings_keep_current_fact_when_history_is_missing_foreign_corrupt_or_unsupported() {
    run_test(async {
        let fixture = Fixture::stopped();
        for (mode, expected_state) in [
            ("missing", BindingHistoryState::Unavailable),
            ("foreign", BindingHistoryState::Incomplete),
            ("unsupported", BindingHistoryState::Unsupported),
            ("corrupt", BindingHistoryState::Incomplete),
        ] {
            if mode != "missing" {
                let mut record = meta(&fixture, "old-provider");
                if mode == "foreign" {
                    record.session_id = "someone-else".into();
                }
                if mode == "unsupported" {
                    record.schema_version += 1;
                }
                history(&fixture, vec![HistoryRecord::Meta(record)]);
                if mode == "corrupt" {
                    std::fs::write(
                        ora_history::history_path(
                            &fixture._temporary.path().join("sessions"),
                            "session-1",
                        )
                        .unwrap(),
                        b"broken\n",
                    )
                    .unwrap();
                }
            }
            assert_eq!(
                fixture
                    .sessions
                    .bindings(GetSessionBindingsRequest {
                        session_ids: vec!["session-1".into()]
                    })
                    .await
                    .unwrap(),
                GetSessionBindingsResponse {
                    sessions: vec![SessionBindings::Available {
                        session_id: "session-1".into(),
                        history: expected_state,
                        segments: vec![SessionBindingSegment {
                            key: "live".into(),
                            agent_ref: "ora-space.fixture".into(),
                            agent_session_id: "provider-session".into(),
                            recorded_at: None,
                            current: true
                        }]
                    }]
                }
            );
        }
        assert!(
            fixture
                .sessions
                .bindings(GetSessionBindingsRequest {
                    session_ids: (0..33).map(|n| n.to_string()).collect()
                })
                .await
                .is_err()
        );
    });
}

#[test]
fn bindings_append_live_row_instead_of_marking_an_older_matching_segment() {
    run_test(async {
        let fixture = Fixture::stopped();
        let mut segments = history(
            &fixture,
            vec![
                HistoryRecord::Meta(meta(&fixture, "provider-session")),
                HistoryRecord::AgentSwitched(AgentSwitch {
                    from: AgentRef::parse("ora-space.fixture").unwrap(),
                    to: AgentRef::parse("ora-space.other").unwrap(),
                    agent_session_id: "other".into(),
                }),
            ],
        );
        segments.push(SessionBindingSegment {
            key: "live".into(),
            agent_ref: "ora-space.fixture".into(),
            agent_session_id: "provider-session".into(),
            recorded_at: None,
            current: true,
        });
        assert_eq!(
            fixture
                .sessions
                .bindings(GetSessionBindingsRequest {
                    session_ids: vec!["session-1".into()]
                })
                .await
                .unwrap(),
            GetSessionBindingsResponse {
                sessions: vec![SessionBindings::Available {
                    session_id: "session-1".into(),
                    history: BindingHistoryState::Readable,
                    segments
                }]
            }
        );
    });
}

/// Mutates the actual row during the read boundary, without timing-dependent sleeps or threads.
#[test]
fn binding_reads_retry_once_and_refuse_a_continuously_changing_row() {
    run_test(async {
        let fixture = Fixture::stopped();
        for keep_changing in [false, true] {
            let mut reads = 0;
            let result = super::super::bindings::read_consistent(
                &fixture.sessions.repository,
                "session-1",
                |session| {
                    reads += 1;
                    if reads == 1 || keep_changing {
                        fixture
                            .sessions
                            .repository
                            .update_session_binding(
                                &session.id,
                                session.agent_ref.clone(),
                                &format!("changed-{reads}"),
                                session.audit_fields.updated_at + 1,
                            )
                            .unwrap();
                    }
                    (BindingHistoryState::Readable, Vec::new())
                },
            );
            assert_eq!(reads, 2);
            if keep_changing {
                assert_eq!(
                    result,
                    SessionBindings::Retry {
                        session_id: "session-1".into()
                    }
                );
            } else {
                assert_eq!(
                    result,
                    SessionBindings::Available {
                        session_id: "session-1".into(),
                        history: BindingHistoryState::Readable,
                        segments: vec![SessionBindingSegment {
                            key: "live".into(),
                            agent_ref: "ora-space.fixture".into(),
                            agent_session_id: "changed-1".into(),
                            recorded_at: None,
                            current: true
                        }]
                    }
                );
            }
        }
    });
}

#[test]
fn binding_reads_distinguish_uncreated_from_truncated_history() {
    run_test(async {
        let fixture = Fixture::stopped();
        let repository = &fixture.sessions.repository;
        repository
            .update_session_binding(
                &SessionId::new("session-1"),
                AgentRef::parse("ora-space.fixture").unwrap(),
                "",
                2,
            )
            .unwrap();
        history(&fixture, Vec::new());
        assert_eq!(
            fixture
                .sessions
                .bindings(GetSessionBindingsRequest {
                    session_ids: vec!["session-1".into()]
                })
                .await
                .unwrap(),
            GetSessionBindingsResponse {
                sessions: vec![SessionBindings::Available {
                    session_id: "session-1".into(),
                    history: BindingHistoryState::Readable,
                    segments: Vec::new()
                }]
            }
        );
        let path =
            ora_history::history_path(&fixture._temporary.path().join("sessions"), "session-1")
                .unwrap();
        std::fs::write(&path, vec![b' '; 8 * 1024 * 1024 + 1]).unwrap();
        assert_eq!(
            fixture
                .sessions
                .bindings(GetSessionBindingsRequest {
                    session_ids: vec!["session-1".into()]
                })
                .await
                .unwrap(),
            GetSessionBindingsResponse {
                sessions: vec![SessionBindings::Available {
                    session_id: "session-1".into(),
                    history: BindingHistoryState::Incomplete,
                    segments: Vec::new()
                }]
            }
        );
    });
}
