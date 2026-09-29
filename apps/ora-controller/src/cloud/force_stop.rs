//! Independent force-stop delivery continues while an ordinary lifecycle round waits on its Node.
//! Cloud persists exact targets before this worker; it never invents execution completion.
use super::{CloudStore, fault, fleet::Fleet, substrate::Observation};
use crate::Error;
use ora_controller_proto::v1::{
    self as proto, effect_evidence::Evidence,
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
            let permit = store.effect_permit(epoch, &effect.id, &plan.id).await?;
            let observed = fleet
                .deployment()
                .substrate
                .execute_authorized(&effect, &permit)
                .await
                .map_err(|e| Error::Io(std::io::Error::other(e.to_string())))?;
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
