//! Lossless subscriptions to the notifications of single plugin process generations.

use crate::connection::PluginGenerationKey;
use crate::ports::{InboundNotification, PluginNotificationSink};
use ora_domain::PluginId;
use std::sync::{Arc, Mutex, PoisonError};
use tokio::sync::mpsc;

/// Routes each notification to the taps opened on its plugin generation.
///
/// Consumers that cannot tolerate a dropped frame, such as an agent connection reading ACP, tap
/// exactly one generation through an unbounded channel: the process runtime already buffers
/// unboundedly, so a tap adds no new loss point, and the lifecycle's pump never waits on it. A
/// restarted plugin gets a new generation, so its frames can never reach a tap opened on its
/// predecessor.
#[derive(Clone, Debug, Default)]
pub struct GenerationTaps {
    taps: Arc<Mutex<Vec<NotificationTap>>>,
}

/// One lossless subscription to the notifications of a single process generation.
#[derive(Debug)]
struct NotificationTap {
    plugin_id: PluginId,
    generation: PluginGenerationKey,
    sender: mpsc::UnboundedSender<InboundNotification>,
}

impl GenerationTaps {
    /// Opens a lossless receiver of every notification `generation` of `plugin_id` emits from now
    /// on. The tap is released when its receiver is dropped.
    pub fn tap(
        &self,
        plugin_id: &PluginId,
        generation: PluginGenerationKey,
    ) -> mpsc::UnboundedReceiver<InboundNotification> {
        let (sender, receiver) = mpsc::unbounded_channel();
        self.taps
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(NotificationTap {
                plugin_id: plugin_id.clone(),
                generation,
                sender,
            });
        receiver
    }

    /// Delivers one notification to the taps of its generation.
    ///
    /// Taps whose receiver went away are pruned as they are encountered, so an abandoned
    /// connection never makes later deliveries fail.
    pub fn deliver(&self, notification: &InboundNotification) {
        self.taps
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .retain(|tap| {
                if tap.plugin_id != notification.plugin_id
                    || tap.generation != notification.generation
                {
                    return !tap.sender.is_closed();
                }
                tap.sender.send(notification.clone()).is_ok()
            });
    }

    /// Counts the taps still registered, for tests that verify release.
    #[cfg(test)]
    fn len(&self) -> usize {
        self.taps
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .len()
    }
}

impl PluginNotificationSink for GenerationTaps {
    /// A host whose only consumers are generation taps uses the registry itself as its sink.
    fn on_notification(&self, notification: InboundNotification) {
        self.deliver(&notification);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use serde_json::json;

    /// Verifies a tap sees only its own plugin generation, in order, and that dropping the
    /// receiver releases the tap instead of failing later publications.
    #[tokio::test]
    async fn taps_receive_only_their_generation_and_release_on_drop() {
        let taps = GenerationTaps::default();
        let plugin_id = PluginId::new("official", "ora-space.agent").expect("plugin id");
        let notification = |generation: u64, method: &str| InboundNotification {
            plugin_id: plugin_id.clone(),
            generation: PluginGenerationKey(generation),
            method: method.to_owned(),
            params: json!({}),
        };
        let mut tap = taps.tap(&plugin_id, PluginGenerationKey(2));
        taps.on_notification(notification(1, "agent/acp"));
        taps.on_notification(notification(2, "agent/acp"));
        taps.on_notification(notification(2, "agent/modelsChanged"));

        let received = (
            tap.recv().await.expect("first"),
            tap.recv().await.expect("second"),
        );
        drop(tap);
        taps.on_notification(notification(2, "agent/acp"));

        assert_eq!(
            (received, taps.len()),
            (
                (
                    notification(2, "agent/acp"),
                    notification(2, "agent/modelsChanged"),
                ),
                0,
            )
        );
    }
}
