//! Ordinary operations renew expired authority without replacing their planned effect.
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
fn authorized_ensure(permit: &proto::RuntimeEffectPermit) -> Value {
    json!({
        "kind": "sandbox_ensure",
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
    })
}

/// A valid permit expires while the journal GET is in flight; a restarted Controller reuses the
/// same effect with fresh authority and performs its only PUT after the retry is offered again.
#[test]
fn ordinary_start_renews_expired_permit_after_restart() {
    scenario("main", |world| async move {
        let (arrived, read) = oneshot::channel();
        let (release, released) = oneshot::channel();
        world.substrate.replies.lock().unwrap().get_barrier = Some((arrived, released));
        // The journal request timeout is two seconds; the barrier consumes this exact lifetime.
        world
            .cloud
            .next_effect_permit_lifetime(Duration::from_secs(/*secs*/ 1));
        let start = world.cloud.queue(proto::OperationKind::Start);
        let effect_id = tokio::time::timeout(Duration::from_secs(/*secs*/ 2), read)
            .await
            .unwrap()
            .unwrap();
        let first = world.cloud.effect_permit_replies();
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
            remaining > 0,
            "the permit must still be valid when GET arrives"
        );
        tokio::time::sleep(Duration::from_millis(remaining as u64)).await;
        assert!(now_ms() >= permit.expires_at_ms);
        release.send(()).unwrap();
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
                proto::OperationState::RetryWait,
                vec![Event::Deferred {
                    reason: "external_failure",
                }],
            )
        );
        assert_eq!(
            world.substrate.replies.lock().unwrap().puts,
            Vec::<(String, Value)>::new()
        );
        let (effects, generation) = world.cloud.effect_identity();
        assert_eq!(
            (
                effects
                    .iter()
                    .map(|effect| effect.id.clone())
                    .collect::<Vec<_>>(),
                generation,
            ),
            (vec![effect_id.clone()], 1)
        );

        world.controller.stop().await;
        world.cloud.reoffer_retries();
        world.controller.start().await;
        settled(&world, &start, proto::OperationState::Succeeded).await;
        let replies = world.cloud.effect_permit_replies();
        assert_eq!(replies.len(), 2);
        assert_eq!(replies[0], first[0]);
        assert_eq!(replies[1].0, request);
        let fresh = &replies[1].1;
        assert!(fresh.issued_at_ms >= permit.expires_at_ms);
        assert!(fresh.expires_at_ms > fresh.issued_at_ms);
        let mut expected_permit = permit.clone();
        expected_permit.issued_at_ms = fresh.issued_at_ms;
        expected_permit.expires_at_ms = fresh.expires_at_ms;
        assert_eq!(*fresh, expected_permit);
        assert_eq!(
            world.substrate.replies.lock().unwrap().puts,
            vec![(effect_id.clone(), authorized_ensure(fresh))]
        );
        let (completed, final_generation) = world.cloud.effect_identity();
        let mut expected = effects;
        expected[0].state = proto::EffectState::Succeeded as i32;
        expected[0].external_id = Some(effect_id.clone());
        expected[0].evidence = Some(proto::EffectEvidence {
            evidence: Some(proto::effect_evidence::Evidence::SandboxEnsured(
                proto::SandboxEnsured {
                    sandbox_instance_id: effect_id,
                    node_id: node_id().as_str().to_owned(),
                },
            )),
        });
        assert_eq!((completed, final_generation), (expected, generation));
    });
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
                authorized_ensure(&replies[0].1)
            )]
        );
        let blocked = world.cloud.operation(&start);
        world.cloud.reoffer_retries();
        assert_eq!(world.cloud.operation(&start), blocked);
    });
}
