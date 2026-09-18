use std::process::ExitCode;

/// Opens a local coordinator for accepted durable commands; no Client or Backend command channel is installed.
fn main() -> ExitCode {
    #[cfg(target_os = "linux")]
    {
        let args: Vec<_> = std::env::args_os().skip(1).collect();
        if args.len() == 1 {
            return match run(std::path::Path::new(&args[0])) {
                Ok(()) => ExitCode::SUCCESS,
                Err(error) => {
                    eprintln!("ora-controller: {error}");
                    ExitCode::FAILURE
                }
            };
        }
    }
    eprintln!("usage (Linux): ora-controller <absolute-config-file>");
    ExitCode::FAILURE
}

/// Reconnects independently per Node, preserving the single durable Controller owner until shutdown.
#[cfg(target_os = "linux")]
fn run(path: &std::path::Path) -> Result<(), Box<dyn std::error::Error>> {
    use ora_controller::*;
    use serde::Deserialize;
    use std::{
        path::PathBuf,
        sync::{Arc, Mutex},
        time::Duration,
    };
    use tokio::signal::unix::{SignalKind, signal};
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Config {
        home_directory: PathBuf,
        protected_state_directories: Vec<PathBuf>,
        controller_id: ora_node_protocol::ControllerId,
        nodes: Vec<NodeEndpoint>,
        session: SessionConfig,
        reconnect_ms: u64,
        timezone: String,
    }
    if !path.is_absolute() {
        return Err("configuration path must be absolute".into());
    }
    let config: Config = serde_json::from_slice(&std::fs::read(path)?)?;
    if config.reconnect_ms == 0
        || config.session.query_interval_ms == 0
        || config.session.io_timeout_ms == 0
    {
        return Err("session intervals must be positive".into());
    }
    let _logging = ora_logging::init_logging(ora_logging::LoggingConfig::new(
        ora_logging::LogLevel::Info,
        ora_logging::LogOutput::Stdout,
        config.timezone.parse()?,
    ))?;
    for (index, target) in config.nodes.iter().enumerate() {
        if target.node_id.as_str().trim().is_empty()
            || !target.endpoint.is_absolute()
            || config.nodes[..index]
                .iter()
                .any(|other| other.node_id == target.node_id || other.endpoint == target.endpoint)
        {
            return Err("duplicate or invalid Node deployment".into());
        }
    }
    let home = ora_utils::path::canonicalize_longest_existing_prefix(&config.home_directory);
    for protected in config
        .protected_state_directories
        .iter()
        .map(PathBuf::as_path)
        .chain(
            config
                .nodes
                .iter()
                .filter_map(|node| node.endpoint.parent()),
        )
    {
        if !protected.is_absolute() {
            return Err("protected state directories must be absolute".into());
        }
        let protected = ora_utils::path::canonicalize_longest_existing_prefix(protected);
        if home.starts_with(&protected) || protected.starts_with(&home) {
            return Err("Controller state overlaps protected deployment state".into());
        }
    }
    let owner = Arc::new(Mutex::new(Controller::open(
        &config.home_directory,
        config.controller_id,
    )?));
    tokio::runtime::Builder::new_current_thread().enable_all().build()?.block_on(async move {
        let mut terminate = signal(SignalKind::terminate())?;
        let mut interrupt = signal(SignalKind::interrupt())?;
        let mut sessions = tokio::task::JoinSet::new();
        for target in config.nodes {
            let owner = owner.clone();
            let settings = config.session.clone();
            sessions.spawn(async move {
                loop {
                    if run_session(&owner, &target, &settings).await.is_err() { ora_logging::ora_warn!(node_id = %target.node_id.as_str(), "Controller connection unavailable; original execution responsibility retained"); }
                    tokio::time::sleep(Duration::from_millis(config.reconnect_ms)).await;
                }
            });
        }
        ora_logging::ora_info!("Controller recovery started");
        tokio::select! { _ = terminate.recv() => {}, _ = interrupt.recv() => {} }
        sessions.abort_all();
        while sessions.join_next().await.is_some() {}
        Ok::<(), std::io::Error>(())
    })?;
    Ok(())
}
