//! Connection-owned lossy relays. One latest snapshot replaces any backlog for each execution.
use super::*;
use std::collections::HashMap;
use tokio::{sync::watch, task::JoinSet};

/// Narrow lossy boundary so a blocked publisher can be tested independently of durable storage.
pub(super) trait PreviewSink: Clone + Send + Sync + 'static {
    /// Publishes optional content without creating a durable receipt.
    fn publish(
        &self,
        node: &NodeRuntimeIdentity,
        preview: &ThreadPreviewMessage,
    ) -> impl std::future::Future<Output = Result<(), Error>> + Send;
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    use tokio::sync::{Semaphore, mpsc};

    #[derive(Clone)]
    struct BlockedSink {
        observed: mpsc::UnboundedSender<u64>,
        gate: Arc<Semaphore>,
        completed: Arc<AtomicUsize>,
    }

    struct Completion(Arc<AtomicUsize>);
    impl Drop for Completion {
        fn drop(&mut self) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }

    impl PreviewSink for BlockedSink {
        async fn publish(
            &self,
            _node: &NodeRuntimeIdentity,
            preview: &ThreadPreviewMessage,
        ) -> Result<(), Error> {
            let _completion = Completion(self.completed.clone());
            self.observed.send(preview.payload.version).unwrap();
            self.gate.acquire().await.unwrap().forget();
            Ok(())
        }
    }

    /// A blocked network call retains only the latest replacement, and disconnect aborts it.
    #[tokio::test]
    async fn slow_publisher_coalesces_and_connection_drop_cancels_workers() {
        let (observed, mut received) = mpsc::unbounded_channel();
        let sink = BlockedSink {
            observed,
            gate: Arc::new(Semaphore::new(0)),
            completed: Arc::new(AtomicUsize::new(0)),
        };
        let node = NodeRuntimeIdentity {
            node_id: NodeId::new("node"),
            incarnation_id: NodeIncarnationId::new("incarnation"),
        };
        let message = |version| ThreadPreviewMessage {
            protocol_version: CURRENT_PROTOCOL_VERSION,
            operation_id: OperationId::new("run"),
            execution_id: ExecutionId::new("execution"),
            payload: ThreadPreview {
                version,
                blocks: Vec::new(),
                incomplete: false,
            },
        };
        let mut relay = Relay::new();
        relay.push(&sink, &node, message(1));
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(1), received.recv())
                .await
                .unwrap(),
            Some(1)
        );
        relay.push(&sink, &node, message(2));
        relay.push(&sink, &node, message(3));
        sink.gate.add_permits(1);
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(1), received.recv())
                .await
                .unwrap(),
            Some(3)
        );
        assert!(received.try_recv().is_err());
        drop(relay);
        tokio::time::timeout(Duration::from_secs(1), async {
            while sink.completed.load(Ordering::SeqCst) != 2 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
    }
}

#[derive(Clone)]
pub(super) struct StoreSink<S>(pub(super) S);

impl<S: CoordinationStore> PreviewSink for StoreSink<S> {
    /// Keeps storage authority checks in its adapter while the connection owns lossy delivery.
    fn publish(
        &self,
        node: &NodeRuntimeIdentity,
        preview: &ThreadPreviewMessage,
    ) -> impl std::future::Future<Output = Result<(), Error>> + Send {
        self.0.publish_thread_preview(node, preview)
    }
}

pub(super) struct Relay {
    queues: HashMap<ExecutionId, watch::Sender<Option<ThreadPreviewMessage>>>,
    tasks: JoinSet<()>,
}

impl Relay {
    /// No worker or content exists until the negotiated Node sends a preview.
    pub(super) fn new() -> Self {
        Self {
            queues: HashMap::new(),
            tasks: JoinSet::new(),
        }
    }

    /// Overflow loses only optional previews. A hung Cloud call cannot backpressure Thread receipts.
    pub(super) fn push<S: PreviewSink>(
        &mut self,
        store: &S,
        node: &NodeRuntimeIdentity,
        preview: ThreadPreviewMessage,
    ) {
        if self.queues.len() >= 64 && !self.queues.contains_key(&preview.execution_id) {
            return;
        }
        let sender = self
            .queues
            .entry(preview.execution_id.clone())
            .or_insert_with(|| {
                let (send, mut receive) = watch::channel::<Option<ThreadPreviewMessage>>(None);
                let store = store.clone();
                let node = node.clone();
                self.tasks.spawn(async move {
                    while receive.changed().await.is_ok() {
                        let Some(snapshot) = receive.borrow_and_update().clone() else {
                            continue;
                        };
                        // Preview failure does not end an Agent or produce a durable acknowledgement.
                        let _ = tokio::time::timeout(
                            Duration::from_secs(2),
                            store.publish(&node, &snapshot),
                        )
                        .await;
                    }
                });
                send
            });
        sender.send_replace(Some(preview));
    }
}
