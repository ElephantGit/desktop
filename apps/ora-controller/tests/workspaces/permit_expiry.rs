//! Ordinary operations renew short-lived authority without replacing their planned effect.
use super::*;
use pretty_assertions::assert_eq;

/// Uses the same absolute clock as the production PUT admission check.
fn now_ms() -> i64 {
    (ora_logging::clock::now_local().unix_timestamp_nanos() / 1_000_000) as i64
}

/// Returns every accepted parking reason, preserving unexpected extra deferrals in comparisons.
fn deferrals(world: &World) -> Vec<Event> {
    world
        .timeline
        .events()
        .into_iter()
        .filter(|event| matches!(event, Event::Deferred { .. }))
        .collect()
}

/// Retains the complete authorization body expected at the ordinary operation's PUT boundary.
fn authorized_request(permit: &proto::RuntimeEffectPermit, kind: proto::EffectKind) -> Value {
    let mut body = json!({
        "kind": workspace_cloud::kind_name(kind),
        "projectId": workspace_cloud::PROJECT,
        "workspaceId": WORKSPACE,
        "authorization": {
            "tenantId": permit.tenant_id,
            "workspaceId": permit.workspace_id,
            "sandboxId": permit.sandbox_id,
            "runtimeGeneration": permit.runtime_generation,
            "controllerEpoch": permit.controller_epoch,
            "controlEpoch": permit.control_epoch,
            "issuedAtMs": permit.issued_at_ms,
            "expiresAtMs": permit.expires_at_ms,
        },
    });
    if kind == proto::EffectKind::SandboxTerminate {
        body["sandboxInstanceId"] = json!(permit.sandbox_id);
    }
    body
}

/// Start renews a permit with insufficient PUT lifetime in the same running Controller.
#[test]
fn ordinary_start_renews_short_lived_permit_without_restart() {
    renew_short_lived_permit(proto::OperationKind::Start);
}

/// DeleteWorkspace renews termination authority without replacing its planned sandbox effect.
#[test]
fn ordinary_delete_renews_short_lived_permit_without_restart() {
    renew_short_lived_permit(proto::OperationKind::DeleteWorkspace);
}

/// Journal I/O consumes the PUT margin; reoffering the retry renews the same effect's authority.
fn renew_short_lived_permit(operation_kind: proto::OperationKind) {
    let test = |world: World| async move {
        if operation_kind == proto::OperationKind::DeleteWorkspace {
            let create = world.cloud.queue(proto::OperationKind::CreateWorkspace);
            settled(&world, &create, proto::OperationState::Succeeded).await;
        }
        let (initial_effects, initial_generation) = world.cloud.effect_identity();
        let permit_offset = world.cloud.effect_permit_replies().len();
        let initial_puts = world.substrate.replies.lock().unwrap().puts.clone();
        let event_offset = world.timeline.events().len();
        let (arrived, read) = oneshot::channel();
        let (release, released) = oneshot::channel();
        world.substrate.replies.lock().unwrap().get_barrier = Some((arrived, released));
        // The GET begins above the two-second margin, then crosses it within its request timeout.
        world
            .cloud
            .next_effect_permit_lifetime(Duration::from_secs(/*secs*/ 4));
        let operation = world.cloud.queue(operation_kind);
        let effect_id = tokio::time::timeout(Duration::from_secs(/*secs*/ 2), read)
            .await
            .unwrap()
            .unwrap();
        let first = world.cloud.effect_permit_replies()[permit_offset..].to_vec();
        assert_eq!(first.len(), 1);
        let (request, permit) = first[0].clone();
        assert_eq!(
            request,
            proto::GetEffectPermitRequest {
                epoch: workspace_cloud::EPOCH,
                effect_id: effect_id.clone(),
                force_stop_id: String::new(),
            }
        );
        let remaining = permit.expires_at_ms - now_ms();
        assert!(
            remaining > 2_000,
            "the permit must have enough PUT lifetime when GET arrives"
        );
        if operation_kind == proto::OperationKind::DeleteWorkspace {
            world.substrate.replies.lock().unwrap().by_id.insert(
                effect_id.clone(),
                (
                    StatusCode::OK,
                    json!({
                        "state": "succeeded",
                        "result": {"terminated": true, "lateEnsureFenced": true},
                    }),
                ),
            );
        }
        tokio::time::sleep(Duration::from_millis((remaining - 2_000) as u64)).await;
        let remaining = permit.expires_at_ms - now_ms();
        assert!(
            remaining > 0 && remaining <= 2_000,
            "GET must finish with a valid permit inside the PUT margin: {remaining}ms"
        );
        release.send(()).unwrap();
        world
            .timeline
            .until(|events| {
                events[event_offset..].iter().any(|event| {
                    matches!(
                        event,
                        Event::Deferred { .. } | Event::Substrate { method: "PUT", .. }
                    )
                })
            })
            .await;
        assert_eq!(
            (world.cloud.operation(&operation).state(), deferrals(&world)),
            (
                proto::OperationState::RetryWait,
                vec![Event::Deferred {
                    reason: "external_failure",
                }],
            )
        );
        assert_eq!(world.substrate.replies.lock().unwrap().puts, initial_puts);
        let (effects, generation) = world.cloud.effect_identity();
        assert_eq!(
            (
                effects
                    .iter()
                    .map(|effect| effect.id.clone())
                    .collect::<Vec<_>>(),
                generation,
            ),
            (
                initial_effects
                    .iter()
                    .map(|effect| effect.id.clone())
                    .chain(std::iter::once(effect_id.clone()))
                    .collect::<Vec<_>>(),
                initial_generation + i64::from(operation_kind == proto::OperationKind::Start),
            )
        );

        world.cloud.reoffer_retries();
        settled(&world, &operation, proto::OperationState::Succeeded).await;
        let replies = world
            .cloud
            .effect_permit_replies()
            .into_iter()
            .filter(|(request, _)| request.effect_id == effect_id)
            .collect::<Vec<_>>();
        assert_eq!(replies.len(), 2);
        assert_eq!(replies[0], first[0]);
        assert_eq!(replies[1].0, request);
        let fresh = &replies[1].1;
        assert!(fresh.issued_at_ms >= permit.expires_at_ms - 2_000);
        assert_eq!(fresh.expires_at_ms - fresh.issued_at_ms, 10_000);
        let mut expected_permit = permit.clone();
        expected_permit.issued_at_ms = fresh.issued_at_ms;
        expected_permit.expires_at_ms = fresh.expires_at_ms;
        assert_eq!(*fresh, expected_permit);
        let target = effects.last().unwrap();
        assert_eq!(
            world
                .substrate
                .replies
                .lock()
                .unwrap()
                .puts
                .iter()
                .filter(|(id, _)| id == &effect_id)
                .cloned()
                .collect::<Vec<_>>(),
            vec![(effect_id.clone(), authorized_request(fresh, target.kind()))]
        );
        let (completed, final_generation) = world.cloud.effect_identity();
        let mut expected = effects;
        let target = expected.last_mut().unwrap();
        target.state = proto::EffectState::Succeeded as i32;
        target.external_id = Some(effect_id.clone());
        target.evidence = Some(proto::EffectEvidence {
            evidence: Some(match operation_kind {
                proto::OperationKind::Start => {
                    proto::effect_evidence::Evidence::SandboxEnsured(proto::SandboxEnsured {
                        sandbox_instance_id: effect_id,
                        node_id: node_id().as_str().to_owned(),
                    })
                }
                proto::OperationKind::DeleteWorkspace => {
                    proto::effect_evidence::Evidence::SandboxTerminated(proto::SandboxTerminated {
                        late_ensure_fenced: true,
                    })
                }
                other => panic!("unsupported recovery operation: {other:?}"),
            }),
        });
        assert_eq!(
            (
                completed
                    .into_iter()
                    .filter(|effect| effect.kind() != proto::EffectKind::WorkspaceDataDelete)
                    .collect::<Vec<_>>(),
                final_generation,
            ),
            (expected, generation)
        );
    };
    scenario_with_substrate_timeout("main", /*request_timeout_ms*/ 5_000, test);
}

/// A permanent Substrate request refusal remains blocked rather than being offered as a retry.
#[test]
fn ordinary_start_keeps_permanent_substrate_rejection_blocked() {
    scenario("main", |world| async move {
        world.substrate.replies.lock().unwrap().ensure =
            Some((StatusCode::CONFLICT, json!({"error": "request_conflict"})));
        let start = world.cloud.queue(proto::OperationKind::Start);
        world
            .timeline
            .until(|events| {
                events
                    .iter()
                    .any(|event| matches!(event, Event::Deferred { .. }))
            })
            .await;
        assert_eq!(
            (world.cloud.operation(&start).state(), deferrals(&world)),
            (
                proto::OperationState::Blocked,
                vec![Event::Deferred {
                    reason: "external_failure",
                }],
            )
        );
        let replies = world.cloud.effect_permit_replies();
        assert_eq!(replies.len(), 1);
        assert_eq!(
            world.substrate.replies.lock().unwrap().puts,
            vec![(
                replies[0].0.effect_id.clone(),
                authorized_request(&replies[0].1, proto::EffectKind::SandboxEnsure)
            )]
        );
        let blocked = world.cloud.operation(&start);
        world.cloud.reoffer_retries();
        assert_eq!(world.cloud.operation(&start), blocked);
    });
}
