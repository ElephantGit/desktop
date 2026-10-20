//! Independent force-stop delivery continues while an ordinary lifecycle round waits on its Node.
//! Cloud persists exact targets before this worker; it never invents execution completion.
use super::{CloudStore, fault, fleet::Fleet, substrate::Observation};
use crate::Error;
use ora_controller_proto::v1::{
    self as proto, effect_evidence::Evidence, effect_request::Request,
    runtime_control_service_client::RuntimeControlServiceClient,
};

impl CloudStore {
    pub(super) async fn effect_permit(
        &self,
        epoch: i64,
        effect: &str,
        force: &str,
    ) -> Result<proto::RuntimeEffectPermit, Error> {
        fault::read(async {
            RuntimeControlServiceClient::new(self.inner.channel.clone())
                .get_effect_permit(self.request(proto::GetEffectPermitRequest {
                    epoch,
                    effect_id: effect.into(),
                    force_stop_id: force.into(),
                }))
                .await
        })
        .await
        .map_err(|v| self.settle(v))?
        .permit
        .ok_or(Error::Conflict)
    }
}

/// Delivers independent Cloud targets without letting one rejected target starve the others.
pub(super) async fn drain(store: &CloudStore, fleet: &Fleet, epoch: i64) -> Result<(), Error> {
    let plans = fault::read(async {
        RuntimeControlServiceClient::new(store.inner.channel.clone())
            .list_force_stops(store.request(proto::ListForceStopsRequest { epoch }))
            .await
    })
    .await
    .map_err(|v| store.settle(v))?
    .plans;
    for plan in plans {
        let mut version = plan.version;
        let remaining: Vec<_> = plan
            .effects
            .iter()
            .filter(|e| e.state() != proto::EffectState::Succeeded)
            .cloned()
            .collect();
        if remaining.is_empty() {
            confirm(store, epoch, &plan.id, version, "").await?;
            continue;
        }
        for effect in remaining {
            if store.lease() != Some(epoch) {
                return Err(Error::StaleEligibility);
            }
            let permit = match store.effect_permit(epoch, &effect.id, &plan.id).await {
                Ok(permit) => permit,
                Err(
                    error @ (Error::StaleEligibility | Error::Unavailable(_) | Error::Unknown(_)),
                ) => {
                    return Err(error);
                }
                Err(error) => {
                    ora_logging::ora_warn!(
                        effect_id = %effect.id,
                        error = %error,
                        "force-stop target permit refused; continuing other targets"
                    );
                    continue;
                }
            };
            let observed = fleet
                .deployment()
                .substrate
                .execute_authorized(
                    &effect,
                    &permit,
                    || store.lease().filter(|held| *held == epoch),
                    async {
                        if let Some(Request::SandboxTerminate(request)) = effect
                            .request
                            .as_ref()
                            .and_then(|request| request.request.as_ref())
                        {
                            fleet.remove(&request.sandbox_instance_id).await;
                        }
                    },
                )
                .await;
            let observed = match observed {
                Ok(observed) => observed,
                Err(error) => {
                    if store.lease() != Some(epoch) {
                        return Err(Error::StaleEligibility);
                    }
                    ora_logging::ora_warn!(
                        effect_id = %effect.id,
                        error = %error,
                        "force-stop target remains unconfirmed; continuing other targets"
                    );
                    continue;
                }
            };
            if matches!(
                observed,
                Observation::Succeeded {
                    evidence: proto::EffectEvidence {
                        evidence: Some(Evidence::SandboxTerminated(proto::SandboxTerminated {
                            late_ensure_fenced: true
                        }))
                    },
                    ..
                }
            ) {
                version = confirm(store, epoch, &plan.id, version, &effect.id).await?;
            }
        }
    }
    Ok(())
}

/// Reports only a Substrate dual confirmation, retaining Cloud's plan and version identities.
async fn confirm(
    store: &CloudStore,
    epoch: i64,
    id: &str,
    version: i64,
    effect: &str,
) -> Result<i64, Error> {
    fault::write(|submission_id| async move {
        RuntimeControlServiceClient::new(store.inner.channel.clone())
            .confirm_force_stop(store.request(proto::ConfirmForceStopRequest {
                submission_id,
                epoch,
                force_stop_id: id.into(),
                version,
                effect_id: effect.into(),
                terminated: !effect.is_empty(),
                late_ensure_fenced: !effect.is_empty(),
            }))
            .await
    })
    .await
    .map(|v| v.version)
    .map_err(|v| store.settle(v))
}
