//! Bootstrap-only guardian: persistent identity and authenticated read-only readiness, no Runs.

mod journal;

use std::fs::File;
use std::future::Future;
use std::io;
use std::os::unix::fs::PermissionsExt;
use std::time::Duration;

use ora_process_protocol::{
    GUARDIAN_MAX_FRAME, GUARDIAN_WIRE_VERSION, GuardianAccess, GuardianBootstrap, GuardianChannel,
    GuardianReady, GuardianReadyRequest, decode_guardian_payload, encode_guardian_frame,
};
use ora_utils::fs::LinuxFileLock;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{UnixListener, UnixStream};
use tokio::task::JoinSet;

use crate::ProcessStateError;

/// Owns an inherited locked description before any journal creation or endpoint publication.
///
/// Bootstrap EOF after delivery is not a liveness lease. All three sockets currently support only
/// authenticated readiness queries; no mutation, event subscription or workload I/O is available.
pub async fn serve_guardian_bootstrap(
    inherited_lock: File,
    mut bootstrap: UnixStream,
    shutdown: impl Future<Output = ()>,
) -> Result<(), ProcessStateError> {
    // SAFETY: geteuid only queries the effective process identity.
    let owner = unsafe { libc::geteuid() };
    if bootstrap.peer_cred()?.uid() != owner {
        return Err(ProcessStateError::Rejected(
            "bootstrap peer identity mismatch",
        ));
    }
    let lock = LinuxFileLock::adopt_inherited(inherited_lock)?;
    let message: GuardianBootstrap = tokio::time::timeout(
        Duration::from_secs(/*secs*/ 5),
        read_message(&mut bootstrap),
    )
    .await
    .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "bootstrap timed out"))??;
    if message.version != GUARDIAN_WIRE_VERSION {
        return Err(ProcessStateError::Rejected(
            "unsupported guardian bootstrap version",
        ));
    }
    let access = message.access;
    drop(bootstrap);
    let _journal = journal::initialize(&access, &lock, owner)?;
    let control = UnixListener::bind(
        access
            .scope_dir
            .join(GuardianChannel::Control.socket_name()),
    )?;
    let events = UnixListener::bind(access.scope_dir.join(GuardianChannel::Events.socket_name()))?;
    let io_listener = UnixListener::bind(access.scope_dir.join(GuardianChannel::Io.socket_name()))?;
    for channel in [
        GuardianChannel::Control,
        GuardianChannel::Events,
        GuardianChannel::Io,
    ] {
        std::fs::set_permissions(
            access.scope_dir.join(channel.socket_name()),
            std::fs::Permissions::from_mode(/*mode*/ 0o600),
        )?;
    }
    File::open(&access.scope_dir)?.sync_all()?;
    let mut control_workers = JoinSet::new();
    let mut event_workers = JoinSet::new();
    let mut io_workers = JoinSet::new();
    tokio::pin!(shutdown);
    let result = loop {
        tokio::select! {
            biased;
            () = &mut shutdown => break Ok(()),
            result = control_workers.join_next(), if !control_workers.is_empty() => {
                if let Some(Err(error)) = result { break Err(io::Error::other(error).into()); }
            }
            result = event_workers.join_next(), if !event_workers.is_empty() => {
                if let Some(Err(error)) = result { break Err(io::Error::other(error).into()); }
            }
            result = io_workers.join_next(), if !io_workers.is_empty() => {
                if let Some(Err(error)) = result { break Err(io::Error::other(error).into()); }
            }
            result = control.accept(), if control_workers.len() < 16 => {
                match result {
                    Ok((stream, _)) => { control_workers.spawn(serve_probe(stream, access.clone(), GuardianChannel::Control, owner)); }
                    Err(error) => break Err(error.into()),
                }
            }
            result = events.accept(), if event_workers.len() < 16 => {
                match result {
                    Ok((stream, _)) => { event_workers.spawn(serve_probe(stream, access.clone(), GuardianChannel::Events, owner)); }
                    Err(error) => break Err(error.into()),
                }
            }
            result = io_listener.accept(), if io_workers.len() < 16 => {
                match result {
                    Ok((stream, _)) => { io_workers.spawn(serve_probe(stream, access.clone(), GuardianChannel::Io, owner)); }
                    Err(error) => break Err(error.into()),
                }
            }
        }
    };
    control_workers.shutdown().await;
    event_workers.shutdown().await;
    io_workers.shutdown().await;
    // Stable lock and endpoints are not unlinked on shutdown; old scope initialization stays closed.
    result
}

/// Rejects a peer before returning any facts; sessions bind the scope, credential and socket role.
async fn serve_probe(
    mut stream: UnixStream,
    access: GuardianAccess,
    channel: GuardianChannel,
    owner: u32,
) -> io::Result<()> {
    tokio::time::timeout(Duration::from_secs(/*secs*/ 5), async {
        if stream.peer_cred()?.uid() != owner {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "guardian peer identity mismatch",
            ));
        }
        let request: GuardianReadyRequest = read_message(&mut stream).await?;
        if request.version != GUARDIAN_WIRE_VERSION
            || request.intent != access.intent
            || request.credential != access.credential
            || request.channel != channel
        {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "guardian probe rejected",
            ));
        }
        let ready = GuardianReady {
            version: GUARDIAN_WIRE_VERSION,
            intent: access.intent,
            channel,
            session: request.session,
        };
        stream.write_all(&encode_guardian_frame(&ready)?).await?;
        Ok(())
    })
    .await
    .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "guardian probe timed out"))?
}

/// Bounds allocation before decoding; the caller supplies the deadline for the whole exchange.
async fn read_message<T: serde::de::DeserializeOwned>(stream: &mut UnixStream) -> io::Result<T> {
    let length = stream.read_u32().await? as usize;
    if length > GUARDIAN_MAX_FRAME {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "guardian frame exceeds limit",
        ));
    }
    let mut bytes = vec![0; length];
    stream.read_exact(&mut bytes).await?;
    decode_guardian_payload(&bytes)
}
