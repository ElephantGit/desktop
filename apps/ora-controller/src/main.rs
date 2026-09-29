use std::process::ExitCode;

#[cfg(target_os = "linux")]
mod cli;

/// Runs the composed Controller executable with executable-owned signals.
fn main() -> ExitCode {
    #[cfg(target_os = "linux")]
    {
        match run() {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("ora-controller: {error}");
                ExitCode::FAILURE
            }
        }
    }
    #[cfg(not(target_os = "linux"))]
    {
        eprintln!("ora-controller currently requires the Linux local Node runtime");
        ExitCode::FAILURE
    }
}

/// Validates flags and configuration before opening state; stops only this composition on shutdown.
/// The persistence kind selects the adapter; neither composition opens a listener.
#[cfg(target_os = "linux")]
fn run() -> Result<(), Box<dyn std::error::Error>> {
    use clap::Parser;
    use ora_controller::{CloudStore, DeploymentConfig, Persistence, Service, SqliteStore};
    let cli = cli::Cli::parse();
    if !cli.config.is_absolute() {
        return Err("configuration path must be absolute".into());
    }
    let hosting = cli.hosting();
    let config: DeploymentConfig = serde_json::from_slice(&std::fs::read(&cli.config)?)?;
    if let Persistence::Cloud {
        endpoint,
        substrate,
        ..
    } = &config.controller.persistence
    {
        if config.controller.management_tls.is_none() || !endpoint.starts_with("https://") {
            return Err(
                "Cloud management requires mutual TLS certificate files and https:// endpoint"
                    .into(),
            );
        }
        if substrate
            .as_ref()
            .is_some_and(|s| s.direct_node_port.is_none() || !s.effects_url.starts_with("https://"))
        {
            return Err(
                "Cloud sandboxes require authenticated HTTPS effects and direct Node TLS endpoints"
                    .into(),
            );
        }
    }
    let _logging = ora_logging::init_logging(ora_logging::LoggingConfig::new(
        ora_logging::LogLevel::Info,
        ora_logging::LogOutput::Stdout,
        config.controller.timezone.parse()?,
    ))?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    match &config.controller.persistence {
        Persistence::Sqlite => {
            let home = config.controller.home_directory.clone();
            runtime.block_on(async {
                let service = Service::<SqliteStore>::start(config, hosting).await?;
                // Through the logger rather than println!, so the line keeps its place among the
                // events written by the logger's own thread; launchers and tests read it there.
                ora_logging::ora_info!(home = %home.display(), "ora-controller coordinating locally");
                serve(service).await
            })
        }
        Persistence::Cloud { endpoint, .. } => {
            let endpoint = endpoint.clone();
            runtime.block_on(async {
                let service = Service::<CloudStore>::start(config, hosting).await?;
                ora_logging::ora_info!(endpoint = %endpoint, "ora-controller coordinating through cloud");
                serve(service).await
            })
        }
    }
}

/// Runs one composition until the executable-owned termination signals fire.
#[cfg(target_os = "linux")]
async fn serve<S: ora_controller::CoordinationStore>(
    service: ora_controller::Service<S>,
) -> Result<(), Box<dyn std::error::Error>> {
    use tokio::signal::unix::{SignalKind, signal};
    let mut terminate = signal(SignalKind::terminate())?;
    let mut interrupt = signal(SignalKind::interrupt())?;
    service
        .run(async {
            tokio::select! { _ = terminate.recv() => {}, _ = interrupt.recv() => {} }
        })
        .await?;
    Ok(())
}
