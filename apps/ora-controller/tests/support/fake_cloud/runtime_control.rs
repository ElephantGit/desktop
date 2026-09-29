//! Simulated Cloud permits test coordination only; real PostgreSQL and Node tests enforce fences.
use super::*;
use ora_controller_proto::v1::runtime_control_service_server::RuntimeControlService;
impl FakeCloud {
    fn runtime_bindings(&self) -> Vec<proto::RuntimeBinding> {
        vec![]
    }
}
#[tonic::async_trait]
impl RuntimeControlService for FakeCloud {
    async fn list_bindings(
        &self,
        _: Request<proto::ListBindingsRequest>,
    ) -> Result<Response<proto::ListBindingsResponse>, Status> {
        Ok(Response::new(proto::ListBindingsResponse {
            bindings: self.runtime_bindings(),
        }))
    }
    async fn acknowledge_binding(
        &self,
        _: Request<proto::AcknowledgeBindingRequest>,
    ) -> Result<Response<proto::AcknowledgeBindingResponse>, Status> {
        Ok(Response::new(proto::AcknowledgeBindingResponse {}))
    }
    async fn get_execution_permit(
        &self,
        request: Request<proto::GetExecutionPermitRequest>,
    ) -> Result<Response<proto::GetExecutionPermitResponse>, Status> {
        let _ = request;
        Err(Status::aborted("runtime_control_required"))
    }
    async fn get_effect_permit(
        &self,
        request: Request<proto::GetEffectPermitRequest>,
    ) -> Result<Response<proto::GetEffectPermitResponse>, Status> {
        let _ = request;
        Err(Status::aborted("executor_capability_unavailable"))
    }
    async fn list_force_stops(
        &self,
        _: Request<proto::ListForceStopsRequest>,
    ) -> Result<Response<proto::ListForceStopsResponse>, Status> {
        Ok(Response::new(proto::ListForceStopsResponse {
            plans: vec![],
        }))
    }
    async fn confirm_force_stop(
        &self,
        _: Request<proto::ConfirmForceStopRequest>,
    ) -> Result<Response<proto::ConfirmForceStopResponse>, Status> {
        Err(Status::failed_precondition("no_force_intent"))
    }
}
