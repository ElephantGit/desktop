use crate::*;
use std::future::Future;

/// The durable coordination boundary between clone coordination logic and whichever authority
/// persists it: the local SQLite adapter today, a Cloud RPC adapter for cloud deployments later.
///
/// Every method is one atomic business operation that the implementation commits as a whole; the
/// trait deliberately exposes no transaction, connection or table so a remote adapter can honor the
/// same promises with a single request. Implementations are cheap to clone and shared across the
/// runtime, the Node sessions and the API. The ordering promises are the contract, not a hint:
///
/// - `accept_request` returns only after the original request, its identities and the full input
///   are durable; the same request with the same input returns the original record, a different
///   input is a conflict.
/// - `take_over_node_event` returns only after the execution fact and the exact event receipt are
///   durable; callers acknowledge that sequence to the Node only after it succeeds.
/// - `record_queried_result` stores a result learned by querying the Node and never produces a
///   receipt, so it can never justify an acknowledgement.
/// - Reads distinguish an unknown identity (conflict or `None`) from an accepted execution whose
///   result is not known yet.
pub trait CoordinationStore: Clone + Send + Sync + 'static {
    /// The persistent coordinator identity presented to Nodes; never a process or connection identity.
    fn id(&self) -> &ControllerId;

    /// Freezes caller intent and the original dispatch identities before any network operation.
    fn accept_request(
        &self,
        request: RequestId,
        spec: CloneExecutionSpec,
    ) -> impl Future<Output = Result<CloneRepositoryMessage, Error>> + Send;

    /// Commits the execution fact carried by a Node event together with its exact receipt.
    fn take_over_node_event(
        &self,
        session: &NodeRuntimeIdentity,
        event: &CloneResultMessage,
    ) -> impl Future<Output = Result<(), Error>> + Send;

    /// Commits a Completed result learned by query; identical facts are idempotent, differing ones conflict.
    fn record_queried_result(
        &self,
        session: &NodeRuntimeIdentity,
        operation: &OperationId,
        execution: &ExecutionId,
        result: &CloneExecutionResult,
    ) -> impl Future<Output = Result<(), Error>> + Send;

    /// Resolves the exact original command before any remote fact is trusted or retransmitted.
    fn original_dispatch(
        &self,
        session: &NodeRuntimeIdentity,
        operation: &OperationId,
        execution: &ExecutionId,
    ) -> impl Future<Output = Result<CloneRepositoryMessage, Error>> + Send;

    /// Lists the commands dispatched to one Node that have no durable result yet: the executions a
    /// session keeps querying after reconnecting. Completed executions leave this list; their replayed
    /// events are still verified through `original_dispatch`, so nothing is lost by not polling them.
    fn pending_dispatches(
        &self,
        node: &NodeId,
    ) -> impl Future<Output = Result<Vec<CloneRepositoryMessage>, Error>> + Send;

    /// Reads the durable terminal result without acknowledging anything.
    fn result(
        &self,
        execution: &ExecutionId,
    ) -> impl Future<Output = Result<Option<CloneExecutionResult>, Error>> + Send;

    /// Lists accepted operations for presentation, newest first.
    fn operations(&self) -> impl Future<Output = Result<Vec<CloneOperation>, Error>> + Send;

    /// Reads one operation; `None` is an absent identity, not a pending result.
    fn operation(
        &self,
        execution: &ExecutionId,
    ) -> impl Future<Output = Result<Option<CloneOperation>, Error>> + Send;
}
