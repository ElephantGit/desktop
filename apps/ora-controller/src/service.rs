use super::*;
use single_node::ManagedNode;
use std::{future::Future, io};
use tokio::{sync::watch, task::JoinHandle};

/// One process hosting the sole Controller owner and optionally its Node. It opens no listener in
/// any persistence mode: a cloud deployment dials out to Cloud, and a local one accepts work only
/// through the embedding [`ControllerHandle`].
pub struct Service<S: CoordinationStore> {
    runtime: ControllerRuntime<S>,
    managed: Option<ManagedNode>,
}

impl Service<SqliteStore> {
    /// Validates composition, opens the exclusive local owner, then hosts the Node when requested.
    pub async fn start(config: DeploymentConfig, hosting: NodeHosting) -> Result<Self, Error> {
        let single = config.validate(hosting)?.cloned();
        let (runtime, managed) = compose(
            &config,
            single.as_ref(),
            ControllerRuntime::<SqliteStore>::open,
        )
        .await?;
        Ok(Self { runtime, managed })
    }
}

impl Service<CloudStore> {
    /// Composes a cloud deployment: the Controller bound to Cloud and optionally its Node.
    /// Acceptance and queries belong to Cloud's public API.
    pub async fn start(config: DeploymentConfig, hosting: NodeHosting) -> Result<Self, Error> {
        let single = config.validate(hosting)?.cloned();
        let (runtime, managed) = compose(
            &config,
            single.as_ref(),
            ControllerRuntime::<CloudStore>::open,
        )
        .await?;
        Ok(Self { runtime, managed })
    }
}

/// The order every composition shares: all checks, then the owner, then the hosted Node, so a
/// refusal leaves no state behind and a Node never runs without its Controller.
async fn compose<S: CoordinationStore>(
    config: &DeploymentConfig,
    single: Option<&SingleNodeConfig>,
    open: impl FnOnce(RuntimeConfig) -> Result<ControllerRuntime<S>, Error>,
) -> Result<(ControllerRuntime<S>, Option<ManagedNode>), Error> {
    let launch = match single {
        Some(single) => Some(ManagedNode::prepare(single, &config.controller).await?),
        None => None,
    };
    let runtime = open(config.controller.clone())?;
    let managed = match launch {
        Some(launch) => Some(launch.start().await?),
        None => None,
    };
    Ok((runtime, managed))
}

impl<S: CoordinationStore> Service<S> {
    /// Runs until shutdown or an unexpected component stop, then stops in order: Node sessions,
    /// the hosted Node, and finally the owner's lease when the runtime drops.
    pub async fn run(self, shutdown: impl Future<Output = ()>) -> io::Result<()> {
        let (stop_sessions, sessions_stopping) = watch::channel(false);
        let runtime = self.runtime;
        let mut sessions: JoinHandle<(ControllerRuntime<S>, io::Result<()>)> =
            tokio::spawn(async move {
                let mut stopping = sessions_stopping;
                let result = runtime
                    .run(async move {
                        let _ = stopping.changed().await;
                    })
                    .await;
                (runtime, result)
            });
        let mut managed = self.managed;
        let (mut sessions_done, mut node_gone) = (false, false);
        let mut runtime = None;
        tokio::pin!(shutdown);
        let outcome = tokio::select! {
            _ = &mut shutdown => Ok(()),
            result = &mut sessions => {
                sessions_done = true;
                // A panicked session task loses the runtime handle; ordered stop still proceeds.
                let detail = match result {
                    Ok((stopped, result)) => {
                        runtime = Some(stopped);
                        format!("{result:?}")
                    }
                    Err(join) => join.to_string(),
                };
                Err(io::Error::other(format!("Controller sessions stopped: {detail}")))
            }
            status = exited(&mut managed), if managed.is_some() => {
                node_gone = true;
                Err(io::Error::other(format!("managed Node exited unexpectedly: {status:?}")))
            }
        };
        // Each phase logs its completion so operators and tests can verify the stop order.
        let _ = stop_sessions.send(true);
        let sessions = if sessions_done {
            Ok(())
        } else {
            let (stopped, result) = sessions.await.map_err(io::Error::other)?;
            runtime = Some(stopped);
            result
        };
        ora_logging::ora_info!("Node sessions stopped");
        let node = match managed {
            Some(node) if !node_gone => {
                let stopped = node.stop().await;
                ora_logging::ora_info!("managed Node stopped");
                stopped
            }
            Some(_) | None => Ok(()),
        };
        // Release the lease only after the hosted Node has been asked to stop.
        drop(runtime);
        outcome.and(sessions).and(node)
    }
}

/// Observes a hosted Node's own exit; callers guard the branch so an external Node never resolves it.
async fn exited(managed: &mut Option<ManagedNode>) -> io::Result<std::process::ExitStatus> {
    match managed {
        Some(node) => node.exited().await,
        None => std::future::pending().await,
    }
}
