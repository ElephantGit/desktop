//! PUT-admission tests use the production HTTP client and a barrier across journal I/O.
#![allow(clippy::unwrap_used)]
use super::*;
use axum::{
    Json, Router,
    extract::State,
    http::{Method, StatusCode},
    routing::any,
};
use pretty_assertions::assert_eq;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicI64, Ordering},
};
use tokio::sync::Notify;

/// A journal read barrier and exact bodies observed at the mutation boundary.
#[derive(Clone, Default)]
struct Fixture {
    arrived: Arc<Notify>,
    release: Arc<Notify>,
    puts: Arc<Mutex<Vec<Value>>>,
}

/// Holds the GET until the test has changed local authority, then answers a typed mutation.
async fn respond(
    State(fixture): State<Fixture>,
    method: Method,
    body: Option<Json<Value>>,
) -> (StatusCode, Json<Value>) {
    if method == Method::GET {
        fixture.arrived.notify_one();
        fixture.release.notified().await;
        return (StatusCode::NOT_FOUND, Json(Value::Null));
    }
    let Json(body) = body.unwrap();
    fixture.puts.lock().unwrap().push(body.clone());
    let result = match body["kind"].as_str().unwrap() {
        "sandbox_terminate" => json!({"terminated":true,"lateEnsureFenced":true}),
        "workspace_data_delete" => json!({"removed":true}),
        kind => panic!("unexpected mutation {kind}"),
    };
    (
        StatusCode::OK,
        Json(json!({"externalId":"effect","state":"succeeded","result":result})),
    )
}

/// Builds the two production destructive requests without changing their stable identities.
fn destructive(kind: proto::EffectKind) -> proto::Effect {
    let request = match kind {
        proto::EffectKind::SandboxTerminate => {
            Request::SandboxTerminate(proto::SandboxTerminateRequest {
                project_id: "project".into(),
                workspace_id: "workspace".into(),
                sandbox_instance_id: "sandbox".into(),
            })
        }
        proto::EffectKind::WorkspaceDataDelete => {
            Request::WorkspaceDataDelete(proto::WorkspaceDataDeleteRequest {
                project_id: "project".into(),
                workspace_id: "workspace".into(),
            })
        }
        _ => panic!("not a destructive fixture kind"),
    };
    proto::Effect {
        id: "effect".into(),
        kind: kind as i32,
        request: Some(proto::EffectRequest {
            request: Some(request),
        }),
        ..proto::Effect::default()
    }
}

/// Exercises one authority transition without burying its request and assertions in loop nesting.
async fn assert_dispatch_admission(kind: proto::EffectKind, case: &str) {
    let fixture = Fixture::default();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let app = Router::new()
        .route("/effects/{id}", any(respond))
        .with_state(fixture.clone());
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let substrate = Substrate::new(&SubstrateConfig {
        effects_url: format!("http://{address}"),
        router_url: "ws://127.0.0.1:1/ora-node/v1".into(),
        atespace: "local".into(),
        request_timeout_ms: 2000,
    })
    .unwrap();
    let now = now_ms();
    let permit = proto::RuntimeEffectPermit {
        tenant_id: "tenant".into(),
        workspace_id: "workspace".into(),
        sandbox_id: "sandbox".into(),
        runtime_generation: 3,
        controller_epoch: if case == "wrong_epoch" { 2 } else { 1 },
        control_epoch: 4,
        issued_at_ms: now,
        expires_at_ms: if case == "expired" {
            now - 1
        } else {
            now + 60000
        },
    };
    // Equality with the expiry instant is invalid, with no wall-clock sleep.
    assert!(validate_permit(&permit, Some(permit.controller_epoch), permit.expires_at_ms).is_err());
    assert_eq!(
        validate_permit(
            &permit,
            Some(permit.controller_epoch),
            permit.expires_at_ms - 1
        )
        .map_err(|error| error.to_string()),
        Ok(())
    );
    let epoch = Arc::new(AtomicI64::new(/*v*/ 1));
    let worker_epoch = epoch.clone();
    let worker_permit = permit.clone();
    let effect = destructive(kind);
    let worker = tokio::spawn(async move {
        substrate
            .execute_authorized(
                &effect,
                &worker_permit,
                || match worker_epoch.load(Ordering::SeqCst) {
                    0 => None,
                    epoch => Some(epoch),
                },
                async {},
            )
            .await
    });
    fixture.arrived.notified().await;
    if case == "lease_lost_during_get" {
        epoch.store(/*val*/ 0, Ordering::SeqCst);
    }
    fixture.release.notify_one();
    let result = worker.await.unwrap();
    if case == "valid" {
        assert!(matches!(result, Ok(Observation::Succeeded { .. })));
        let mut expected = request(&destructive(kind)).unwrap();
        expected["authorization"] = json!({
            "tenantId": permit.tenant_id,
            "workspaceId": permit.workspace_id,
            "sandboxId": permit.sandbox_id,
            "runtimeGeneration": permit.runtime_generation,
            "controllerEpoch": permit.controller_epoch,
            "controlEpoch": permit.control_epoch,
            "issuedAtMs": permit.issued_at_ms,
            "expiresAtMs": permit.expires_at_ms,
        });
        assert_eq!(*fixture.puts.lock().unwrap(), vec![expected]);
    } else {
        assert!(
            matches!(result, Err(SubstrateError::Rejected(_))),
            "{case}: {result:?}"
        );
        assert_eq!(*fixture.puts.lock().unwrap(), Vec::<Value>::new());
    }
    server.abort();
}

/// Expiry and local lease are checked after GET; valid authorization is transmitted unchanged.
#[test]
fn destructive_put_requires_current_unexpired_permit() {
    ora_logging::initialize_test_clock();
    ora_logging::with_trace_logging(|| {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async {
                for kind in [
                    proto::EffectKind::SandboxTerminate,
                    proto::EffectKind::WorkspaceDataDelete,
                ] {
                    for case in ["expired", "wrong_epoch", "lease_lost_during_get", "valid"] {
                        assert_dispatch_admission(kind, case).await;
                    }
                }
            });
    });
}
