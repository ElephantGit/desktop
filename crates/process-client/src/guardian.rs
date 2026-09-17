use ora_process_protocol::{
    GUARDIAN_MAX_FRAME, GUARDIAN_WIRE_VERSION, GuardianAccess, GuardianChannel, GuardianReady,
    GuardianReadyRequest, decode_guardian_payload, encode_guardian_frame,
};
use std::io;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixStream;

/// Authenticates original-instance readiness without taking over control or authorizing Runs.
pub struct GuardianProbe {
    access: GuardianAccess,
    expected_uid: u32,
    session: [u8; 16],
}

impl GuardianProbe {
    /// Uses explicit OS identity and persisted recovery material supplied by the trusted owner.
    pub fn new(access: GuardianAccess, expected_uid: u32) -> Self {
        Self {
            access,
            expected_uid,
            session: *uuid::Uuid::new_v4().as_bytes(),
        }
    }

    /// Queries one physical channel, enforcing a deadline across connect, framing and reply checks.
    pub async fn ready(&self, channel: GuardianChannel) -> io::Result<GuardianReady> {
        tokio::time::timeout(Duration::from_secs(/*secs*/ 5), async {
            let mut stream =
                UnixStream::connect(self.access.scope_dir.join(channel.socket_name())).await?;
            if stream.peer_cred()?.uid() != self.expected_uid {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "guardian OS identity mismatch",
                ));
            }
            let request = GuardianReadyRequest {
                version: GUARDIAN_WIRE_VERSION,
                intent: self.access.intent.clone(),
                credential: self.access.credential.clone(),
                channel,
                session: self.session,
            };
            stream.write_all(&encode_guardian_frame(&request)?).await?;
            let length = stream.read_u32().await? as usize;
            if length > GUARDIAN_MAX_FRAME {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "guardian frame exceeds limit",
                ));
            }
            let mut bytes = vec![0; length];
            stream.read_exact(&mut bytes).await?;
            let ready: GuardianReady = decode_guardian_payload(&bytes)?;
            let expected = GuardianReady {
                version: GUARDIAN_WIRE_VERSION,
                intent: self.access.intent.clone(),
                channel,
                session: self.session,
            };
            if ready != expected {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "guardian Ready identity mismatch",
                ));
            }
            Ok(ready)
        })
        .await
        .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "guardian Ready timed out"))?
    }
}
