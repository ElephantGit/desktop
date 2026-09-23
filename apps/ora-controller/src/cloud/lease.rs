//! The adapter's own coordination with Cloud: hold the global lease so writes are fenced, and
//! turn work Cloud accepted into registered dispatches the Node sessions then deliver. Cloud stays
//! the authority throughout; this loop only decides when to ask.
use super::{CloudStore, fault, mapping};
use crate::*;
use ora_controller_proto::v1 as proto;
use std::{io, time::Duration};
use tokio::time::{MissedTickBehavior, interval};

/// Cloud grants thirty seconds per lease; renewing every ten leaves two missed renewals of slack.
const RENEW_INTERVAL: Duration = Duration::from_secs(/*secs*/ 10);

/// Keeps this Controller eligible and fed until `shutdown` resolves, then releases the lease so a
/// successor need not wait for expiry. Every failure is logged and retried on the next tick; the
/// loop never gives up on the authority, because nothing local could take its place.
pub(super) async fn coordinate(
    store: CloudStore,
    shutdown: impl Future<Output = ()>,
) -> io::Result<()> {
    let mut renew = interval(RENEW_INTERVAL);
    renew.set_missed_tick_behavior(MissedTickBehavior::Delay);
    let mut claim = interval(store.inner.claim_interval);
    claim.set_missed_tick_behavior(MissedTickBehavior::Delay);
    let mut last_refusal: Option<String> = None;
    tokio::pin!(shutdown);
    loop {
        tokio::select! {
            _ = &mut shutdown => break,
            _ = renew.tick() => keep_lease(&store).await,
            _ = claim.tick() => {
                if let Ok(epoch) = store.epoch() {
                    claim_once(&store, epoch, &mut last_refusal).await;
                }
            }
        }
    }
    release(&store).await;
    Ok(())
}

/// Acquires the lease when none is held, otherwise renews it; a renewal Cloud refuses as stale is
/// followed by an immediate acquisition attempt so eligibility returns without waiting a tick.
async fn keep_lease(store: &CloudStore) {
    let held = match store.lease() {
        None => None,
        Some(epoch) => match lease_call(store, proto::RenewLeaseRequest { epoch }).await {
            Ok(lease) => Some(lease),
            Err(Error::StaleEligibility) => None,
            Err(error) => {
                ora_logging::ora_warn!(error = %error, "Cloud lease renewal failed; still eligible until it expires");
                return;
            }
        },
    };
    let lease = match held {
        Some(lease) => lease,
        None => match lease_call(store, proto::AcquireLeaseRequest {}).await {
            Ok(lease) => {
                ora_logging::ora_info!(epoch = lease.epoch, "Cloud lease acquired");
                lease
            }
            Err(error) => {
                ora_logging::ora_warn!(error = %error, "Cloud lease not acquired; no work is claimed or written");
                return;
            }
        },
    };
    store.set_lease(Some(lease.epoch));
}

/// Runs one lease RPC; the verdict's side effects apply as for any other call.
async fn lease_call<R: LeaseRequest>(
    store: &CloudStore,
    message: R,
) -> Result<proto::Lease, Error> {
    let call = async {
        let request = store.request(message);
        R::send(store, request).await
    };
    fault::read(call)
        .await
        .map_err(|verdict| store.settle(verdict))?
        .ok_or(Error::Conflict)
}

/// Claims at most one accepted work item and registers its dispatch before any Node sees it. A
/// claim is a pure read; ownership is decided when `RecordDispatch` commits.
async fn claim_once(store: &CloudStore, epoch: i64, last_refusal: &mut Option<String>) {
    let claimed = fault::write(|submission_id| async move {
        let request = store.request(proto::ClaimWorkRequest {
            submission_id,
            epoch,
        });
        store.executions().claim_work(request).await
    })
    .await;
    let item = match claimed {
        Ok(proto::ClaimWorkResponse { item: Some(item) }) => item,
        Ok(proto::ClaimWorkResponse { item: None }) => return,
        Err(verdict) => {
            let error = store.settle(verdict);
            ora_logging::ora_warn!(error = %error, "Cloud work claim failed");
            return;
        }
    };
    match register(store, epoch, &item).await {
        Ok(command) => {
            *last_refusal = None;
            ora_logging::ora_info!(
                operation_id = %command.operation_id.as_str(),
                execution_id = %command.execution_id.as_str(),
                "dispatch recorded with Cloud; the Node session delivers it"
            );
        }
        // The queue head stays the same until Cloud resolves it, so warn once per item rather
        // than once per tick.
        Err(error) if last_refusal.as_deref() != Some(item.operation_id.as_str()) => {
            *last_refusal = Some(item.operation_id.clone());
            ora_logging::ora_warn!(
                operation_id = %item.operation_id,
                error = %error,
                "claimed work could not be registered for dispatch"
            );
        }
        Err(_) => {}
    }
}

/// Freezes the execution identity and full input with Cloud; the Node protocol validates the
/// command first so nothing undispatchable is ever registered.
async fn register(
    store: &CloudStore,
    epoch: i64,
    item: &proto::WorkItem,
) -> Result<CloneRepositoryMessage, Error> {
    let command = CloneRepositoryMessage {
        protocol_version: CURRENT_PROTOCOL_VERSION,
        request_id: None,
        operation_id: OperationId::new(item.operation_id.clone()),
        execution_id: ExecutionId::new(uuid::Uuid::new_v4().to_string()),
        payload: CloneRepository {
            spec: mapping::spec(item.input.clone(), &store.inner.node)?,
        },
    };
    command.validate()?;
    let write = fault::write(|submission_id| {
        let command = &command;
        async move {
            let request = store.request(proto::RecordDispatchRequest {
                submission_id,
                epoch,
                operation_id: command.operation_id.as_str().into(),
                execution_id: command.execution_id.as_str().into(),
                node_id: store.inner.node.as_str().into(),
                input: Some(mapping::input(&command.payload.spec)),
            });
            store.executions().record_dispatch(request).await
        }
    });
    write.await.map_err(|verdict| store.settle(verdict))?;
    Ok(command)
}

/// Gives the lease back on shutdown; failure only means a successor waits for expiry.
async fn release(store: &CloudStore) {
    let Some(epoch) = store.lease() else {
        return;
    };
    store.set_lease(None);
    if let Err(error) = lease_call(store, proto::ReleaseLeaseRequest { epoch }).await {
        ora_logging::ora_warn!(error = %error, "Cloud lease not released; it expires on its own");
    }
}

/// The three lease requests share one call path; each names the client method it belongs to.
trait LeaseRequest: Sized {
    fn send(
        store: &CloudStore,
        request: tonic::Request<Self>,
    ) -> impl Future<Output = Result<tonic::Response<LeaseReply>, tonic::Status>> + Send;
}

/// The lease each reply carries, unwrapped from the reply message that names the call.
type LeaseReply = Option<proto::Lease>;

impl LeaseRequest for proto::AcquireLeaseRequest {
    async fn send(
        store: &CloudStore,
        request: tonic::Request<Self>,
    ) -> Result<tonic::Response<LeaseReply>, tonic::Status> {
        store
            .leases()
            .acquire_lease(request)
            .await
            .map(|response| response.map(|reply| reply.lease))
    }
}

impl LeaseRequest for proto::RenewLeaseRequest {
    async fn send(
        store: &CloudStore,
        request: tonic::Request<Self>,
    ) -> Result<tonic::Response<LeaseReply>, tonic::Status> {
        store
            .leases()
            .renew_lease(request)
            .await
            .map(|response| response.map(|reply| reply.lease))
    }
}

impl LeaseRequest for proto::ReleaseLeaseRequest {
    async fn send(
        store: &CloudStore,
        request: tonic::Request<Self>,
    ) -> Result<tonic::Response<LeaseReply>, tonic::Status> {
        store
            .leases()
            .release_lease(request)
            .await
            .map(|response| response.map(|reply| reply.lease))
    }
}
