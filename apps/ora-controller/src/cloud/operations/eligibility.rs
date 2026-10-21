//! Losing the local lease ends an operation round without parking it through Cloud.
#![allow(clippy::unwrap_used)]

use super::*;
use crate::cloud::substrate::SubstrateError;
use axum::http::Method;
use ora_controller_proto::v1::effect_request::Request;
use pretty_assertions::assert_eq;
use std::sync::Mutex;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

const EPOCH: i64 = 1;
const SUCCESSOR_EPOCH: i64 = 2;

/// Exercises a lease transition during a real journal read and records all attempted requests.
async fn assert_stale_lease_ends_round(next_epoch: Option<i64>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let store = CloudStore::open(&RuntimeConfig {
        management_tls: None,
        home_directory: std::env::temp_dir().join("controller-epoch-loss"),
        persistence: Persistence::Cloud {
            endpoint: endpoint.clone(),
            claim_interval_ms: 100,
            substrate: Some(SubstrateConfig {
                effects_url: endpoint,
                router_url: "ws://127.0.0.1:1/ora-node/v1".into(),
                atespace: "local".into(),
                request_timeout_ms: 2_000,
            }),
        },
        protected_state_directories: Vec::new(),
        controller_id: ControllerId::new("owner"),
        nodes: Vec::new(),
        session: SessionConfig {
            io_timeout_ms: 1_000,
            query_interval_ms: 20,
        },
        reconnect_ms: 50,
        timezone: "Asia/Shanghai".into(),
    })
    .unwrap();
    store.set_lease(Some(EPOCH));
    let calls = Arc::new(Mutex::new(Vec::new()));
    let server_calls = calls.clone();
    let server_store = store.clone();
    let server = tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            // Read raw headers so an accidental gRPC connection's HTTP/2
            // preface is recorded too; an HTTP/1-only router would hide it.
            let mut header = Vec::new();
            while !header.ends_with(b"\r\n\r\n") {
                header.push(socket.read_u8().await.unwrap());
            }
            let header = String::from_utf8(header).unwrap();
            let mut request = header.lines().next().unwrap().split_whitespace();
            let method = Method::from_bytes(request.next().unwrap().as_bytes()).unwrap();
            let path = request.next().unwrap().to_owned();
            server_calls.lock().unwrap().push((method.clone(), path));
            if method == Method::GET {
                server_store.set_lease(next_epoch);
            }
            socket
                .write_all(
                    b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\
                      Connection: close\r\n\r\n",
                )
                .await
                .unwrap();
        }
    });
    let fleet = Fleet::new(
        store.clone(),
        store.inner.sandboxes.as_ref().unwrap().clone(),
    );
    let effect = proto::Effect {
        id: "terminate".into(),
        kind: proto::EffectKind::SandboxTerminate as i32,
        request: Some(proto::EffectRequest {
            request: Some(Request::SandboxTerminate(proto::SandboxTerminateRequest {
                project_id: "project".into(),
                workspace_id: "workspace".into(),
                sandbox_instance_id: "sandbox".into(),
            })),
        }),
        ..proto::Effect::default()
    };
    let now = (ora_logging::clock::now_local().unix_timestamp_nanos() / 1_000_000) as i64;
    let permit = proto::RuntimeEffectPermit {
        tenant_id: "tenant".into(),
        workspace_id: "workspace".into(),
        sandbox_id: "sandbox".into(),
        runtime_generation: 3,
        controller_epoch: EPOCH,
        control_epoch: 4,
        issued_at_ms: now,
        expires_at_ms: now + 60_000,
    };
    let mut round = Round::new(
        &store,
        &fleet,
        EPOCH,
        proto::OperationSnapshot {
            operation: Some(proto::Operation::default()),
            ..proto::OperationSnapshot::default()
        },
    )
    .unwrap();
    let error = fleet
        .deployment()
        .substrate
        .execute_authorized(
            &effect,
            &permit,
            || store.lease().filter(|epoch| *epoch == EPOCH),
            async {},
        )
        .await
        .unwrap_err();
    assert_eq!(error, SubstrateError::EpochMismatch);
    assert!(matches!(
        round.park_on(&effect, &error).await,
        Err(Error::StaleEligibility)
    ));
    assert_eq!(
        *calls.lock().unwrap(),
        vec![(Method::GET, "/effects/terminate".to_owned())]
    );
    server.abort();
}

/// A journal read that loses its lease must neither dispatch nor defer under the old epoch.
#[test]
fn lease_lost_during_journal_get_ends_round_without_put_or_defer() {
    ora_logging::with_trace_logging(|| {
        ora_logging::initialize_test_clock();
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async {
                assert_stale_lease_ends_round(/*next_epoch*/ None).await;
                assert_stale_lease_ends_round(Some(SUCCESSOR_EPOCH)).await;
            });
    });
}
