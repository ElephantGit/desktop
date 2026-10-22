//! Bounded, read-only projection of existing binding facts, owned by the session domain.
use ora_application::SessionRepository;
use ora_contracts::{BindingHistoryState, SessionBindingSegment, SessionBindings};
use ora_db::SqliteSessionRepository;
use ora_domain::{Session, SessionId};
use ora_history::{HistoryLine, HistoryRecord, SCHEMA_VERSION, history_path};
use std::collections::{BTreeMap, HashSet};
use std::fs::File;
use std::io::Read;
use std::path::Path;

const MAX_HISTORY_BYTES: u64 = 8 * 1024 * 1024;

/// Rechecks the row after file IO, retrying once rather than locking a prompt across disk reads.
pub(super) fn read(repository: &SqliteSessionRepository, root: &Path, id: &str) -> SessionBindings {
    read_consistent(repository, id, |session| read_history(root, session))
}

/// Separates the bounded consistency loop from file IO so race outcomes can be exercised directly.
pub(super) fn read_consistent(
    repository: &SqliteSessionRepository,
    id: &str,
    mut history: impl FnMut(&Session) -> (BindingHistoryState, Vec<SessionBindingSegment>),
) -> SessionBindings {
    let session_id = SessionId::new(id);
    for _ in 0..2 {
        let before = match repository.find_session(&session_id) {
            Ok(Some(session)) => session,
            Ok(None) => {
                return SessionBindings::NotFound {
                    session_id: id.into(),
                };
            }
            Err(_) => {
                return SessionBindings::Unavailable {
                    session_id: id.into(),
                };
            }
        };
        let (history, mut segments) = history(&before);
        let after = match repository.find_session(&session_id) {
            Ok(Some(session)) => session,
            Ok(None) => {
                return SessionBindings::NotFound {
                    session_id: id.into(),
                };
            }
            Err(_) => {
                return SessionBindings::Unavailable {
                    session_id: id.into(),
                };
            }
        };
        // Audit timestamps additionally detect A -> B -> A changes when binding ids are reused.
        if before.agent_ref != after.agent_ref
            || before.agent_session_id != after.agent_session_id
            || before.workspace_id != after.workspace_id
            || before.audit_fields != after.audit_fields
        {
            continue;
        }
        if !after.agent_session_id.trim().is_empty() {
            // An incomplete prefix cannot prove which occurrence is current when ids recur.
            match segments.last_mut() {
                Some(last)
                    if history == BindingHistoryState::Readable
                        && last.agent_ref == after.agent_ref.as_str()
                        && last.agent_session_id == after.agent_session_id =>
                {
                    last.current = true
                }
                _ => segments.push(SessionBindingSegment {
                    key: "live".into(),
                    agent_ref: after.agent_ref.to_string(),
                    agent_session_id: after.agent_session_id,
                    recorded_at: None,
                    current: true,
                }),
            }
        }
        return SessionBindings::Available {
            session_id: id.into(),
            history,
            segments,
        };
    }
    SessionBindings::Retry {
        session_id: id.into(),
    }
}

/// Reads a capped prefix and preserves uncertainty instead of exposing parser or path errors.
fn read_history(
    root: &Path,
    session: &Session,
) -> (BindingHistoryState, Vec<SessionBindingSegment>) {
    let unavailable = || (BindingHistoryState::Unavailable, Vec::new());
    let Ok(path) = history_path(root, session.id.as_ref()) else {
        return unavailable();
    };
    let Ok(file) = File::open(path) else {
        return unavailable();
    };
    let mut bytes = Vec::new();
    if file
        .take(MAX_HISTORY_BYTES + 1)
        .read_to_end(&mut bytes)
        .is_err()
    {
        return unavailable();
    }
    let limited = bytes.len() as u64 > MAX_HISTORY_BYTES;
    bytes.truncate(MAX_HISTORY_BYTES as usize);
    project(session, &bytes, limited)
}

/// Validates identity and binding sequence before accepting any historical segment.
fn project(
    session: &Session,
    bytes: &[u8],
    limited: bool,
) -> (BindingHistoryState, Vec<SessionBindingSegment>) {
    let mut state = if limited {
        BindingHistoryState::Incomplete
    } else {
        BindingHistoryState::Readable
    };
    let mut records = BTreeMap::new();
    let mut conflicts = HashSet::new();
    for raw in bytes.split_inclusive(|byte| *byte == b'\n') {
        if raw.iter().all(u8::is_ascii_whitespace) {
            continue;
        }
        if raw.last() != Some(&b'\n') {
            state = BindingHistoryState::Incomplete;
            continue;
        }
        let Ok(line) = serde_json::from_slice::<HistoryLine>(raw) else {
            state = BindingHistoryState::Incomplete;
            continue;
        };
        if matches!(line.record, HistoryRecord::Gap { .. }) {
            state = BindingHistoryState::Incomplete;
        }
        if let Some(prior) = records.insert(line.seq, line.clone())
            && prior != line
        {
            conflicts.insert(line.seq);
            state = BindingHistoryState::Incomplete;
        }
    }
    let mut segments: Vec<SessionBindingSegment> = Vec::new();
    let mut identity_verified = false;
    for (seq, line) in records {
        if conflicts.contains(&seq) {
            identity_verified = false;
            continue;
        }
        let (agent_ref, agent_session_id) = match line.record {
            HistoryRecord::Meta(meta) => {
                if seq != 0
                    || identity_verified
                    || !segments.is_empty()
                    || meta.session_id != session.id.as_ref()
                    || meta.workspace_id != session.workspace_id.as_ref()
                {
                    return (BindingHistoryState::Incomplete, Vec::new());
                }
                if meta.schema_version != SCHEMA_VERSION {
                    return (BindingHistoryState::Unsupported, Vec::new());
                }
                identity_verified = true;
                (meta.agent_ref, meta.agent_session_id)
            }
            HistoryRecord::AgentSwitched(switch) if identity_verified => {
                if segments
                    .last()
                    .is_none_or(|last| last.agent_ref != switch.from.as_str())
                {
                    state = BindingHistoryState::Incomplete;
                    identity_verified = false;
                    continue;
                }
                (switch.to, switch.agent_session_id)
            }
            HistoryRecord::AgentSwitched(_) => {
                state = BindingHistoryState::Incomplete;
                continue;
            }
            _ => continue,
        };
        if agent_session_id.trim().is_empty() {
            // An initial unbound session is legitimate; a switch without a target is not.
            if seq != 0 {
                state = BindingHistoryState::Incomplete;
            }
            continue;
        }
        let recorded_at =
            time::OffsetDateTime::parse(&line.at, &time::format_description::well_known::Rfc3339)
                .ok()
                .map(|_| line.at);
        if recorded_at.is_none() {
            state = BindingHistoryState::Incomplete;
        }
        segments.push(SessionBindingSegment {
            key: format!("history:{seq}"),
            agent_ref: agent_ref.to_string(),
            agent_session_id,
            recorded_at,
            current: false,
        });
    }
    if !bytes.is_empty() && !identity_verified {
        state = BindingHistoryState::Incomplete;
    }
    (state, segments)
}
