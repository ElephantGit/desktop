use clap::Parser;
use ora_controller::NodeHosting;
use std::path::PathBuf;

/// Composition flags for one process; deployment state stays in the configuration file.
#[derive(Parser, Debug)]
#[command(
    name = "ora-controller",
    about = "Durable clone coordination between an authority and its Nodes"
)]
pub struct Cli {
    /// Absolute path to the deployment configuration file.
    #[arg(long)]
    pub config: PathBuf,
    /// Start the configured Node in this process group and stop it on normal shutdown.
    #[arg(long)]
    pub single_node: bool,
}

impl Cli {
    /// Maps the flag to the explicit hosting choice the service validates against configuration.
    pub fn hosting(&self) -> NodeHosting {
        if self.single_node {
            NodeHosting::Managed
        } else {
            NodeHosting::External
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    /// The hosting flag maps to one explicit choice; there are no listener flags to accept.
    #[test]
    fn flags_map_to_explicit_hosting() {
        let parse = |args: &[&str]| {
            Cli::try_parse_from(["ora-controller", "--config", "/c.json"].iter().chain(args))
        };
        assert_eq!(
            parse(&["--single-node"]).map(|cli| cli.hosting()).ok(),
            Some(NodeHosting::Managed)
        );
        assert_eq!(
            parse(&[]).map(|cli| cli.hosting()).ok(),
            Some(NodeHosting::External)
        );
        assert!(parse(&["--port", "0"]).is_err());
    }
}
