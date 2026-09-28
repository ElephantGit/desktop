//! Session MCP resolver, catalog and configuration sources, and Host health for Desktop.

mod error;
mod health;
mod host;
mod member;
mod resolve;

#[cfg(test)]
mod selection_tests;
#[cfg(test)]
mod tests;

pub(crate) use error::SessionMcpError;
pub(crate) use health::{McpHealthStore, observe_session_mcp_health};
pub(crate) use host::{
    InstalledMcpCandidate, McpConfigurationEligibility, SessionMcpCatalog,
    SessionMcpConfigurationSource, SessionMcpHost,
};
pub(crate) use resolve::{resolve_session_mcp, resolve_session_mcp_revision};

pub(crate) use ora_agent_runtime::{
    AgentSessionMcpCapabilities, SessionMcpMemberRevision, SessionMcpRevision, SessionMcpSnapshot,
    SessionMcpTransportKind,
};
pub(crate) use ora_domain::SessionMcpSelection;
