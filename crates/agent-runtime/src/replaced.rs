//! Repairs live sessions after the agent process behind them was replaced.

use crate::host::RuntimeEvents;
use crate::session_setup::{AgentSessionBarriers, BarrierReason};
use crate::{AgentRuntimeHost, AgentRuntimeManager, RuntimeCommand};
use ora_domain::PluginId;
use ora_logging::ora_debug;
use std::sync::Arc;

/// Repairs live sessions after the agent process behind them was replaced.
///
/// Effect coordination restarts an Agent plugin's process so it re-reads a materialized surface,
/// which silently invalidates every provider-side session that process was holding. Implementations
/// detach those sessions so the next prompt re-establishes them through the ordinary attach path
/// rather than prompting against an id the fresh process cannot resolve.
///
/// The Effect worker depends on this capability rather than on the whole agent runtime, so a
/// reconcile stays exercisable without one.
pub trait ReplacedAgentSessions: Send + Sync + 'static {
    /// Detaches every live session served by the plugin whose agent process was replaced.
    ///
    /// Takes the package address deliberately, rather than the `AgentRef` a Session is bound to. A
    /// plugin carries both identities and Effect only ever holds this one; accepting the other
    /// would let a caller pass an address where an agent name belongs, which compiles, reads
    /// correctly, and silently matches no session at all.
    fn detach_sessions_for_replaced_plugin(&self, plugin_id: &PluginId);

    /// Shared Agent Session Barrier that serializes Effect mutation with MCP refresh.
    fn session_barriers(&self) -> Arc<AgentSessionBarriers>;
}

impl<H: AgentRuntimeHost> ReplacedAgentSessions for AgentRuntimeManager<H> {
    /// One agent's connection is shared by every Workspace, so replacing that process invalidates
    /// sessions well beyond the Workspace whose surface was reconciled. The command is broadcast
    /// and each actor decides whether it is bound to this agent, because the registry is keyed by
    /// Ora session and carries no agent index. Delivery is best effort: an actor that already
    /// ended cannot be holding a stale channel either.
    fn detach_sessions_for_replaced_plugin(&self, plugin_id: &PluginId) {
        let barrier = self.session_barriers().for_plugin(plugin_id);
        let _replacement = barrier.try_acquire(BarrierReason::AgentReplacement);
        ora_debug!(
            plugin_id = %plugin_id,
            barrier_held = barrier.is_held(),
            "detaching sessions after agent process replacement",
        );
        let Some(agent) = self.inner.connections.agent_for_plugin(plugin_id) else {
            // Only an agent-contributing package can have had sessions to lose, so a package that
            // resolves to no agent identity is not a failure — but it is worth saying, because the
            // alternative reading is that the translation itself broke.
            ora_debug!(
                plugin_id = %plugin_id,
                "replaced plugin contributes no agent identity; no session to detach",
            );
            return;
        };
        // The replacement is also the one event that can change what the plugin would answer for
        // this agent, and Ora keeps no model list of its own to correct. Published before the
        // actor sweep so a poisoned registry cannot swallow it: the two are independent repairs.
        self.inner.events.agent_models_invalidated(&agent);
        let Ok(actors) = self.inner.actors.read() else {
            // A poisoned registry means an actor panicked; whatever it held is not trustworthy,
            // and the next interaction rebuilds the session from durable state regardless.
            return;
        };
        for handle in actors.values() {
            let _ = handle.commands.send(RuntimeCommand::AgentProcessReplaced {
                agent: agent.clone(),
            });
        }
    }

    fn session_barriers(&self) -> Arc<AgentSessionBarriers> {
        self.inner.barriers.clone()
    }
}
