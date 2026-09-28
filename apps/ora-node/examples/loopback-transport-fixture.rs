//! A test-only process for transport/crash regressions. Never shipped in a Node image.
//! Production ora-node rejects plaintext; this fixture accepts only unscoped loopback config.
#[cfg(target_os = "linux")]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use tokio::signal::unix::{SignalKind, signal};
    let path = std::env::args_os()
        .nth(1)
        .ok_or("test configuration required")?;
    let config: ora_node::ServiceConfig = serde_json::from_slice(&std::fs::read(path)?)?;
    let control = config.control.as_ref().ok_or("test control required")?;
    if control.target.is_some()
        || !matches!(&control.listen, ora_node::ControlListen::WebSocket { bind, .. } if bind.ip().is_loopback())
    {
        return Err("fixture refuses cloud targets and non-loopback listeners".into());
    }
    let _logging = ora_logging::init_logging(ora_logging::LoggingConfig::new(
        ora_logging::LogLevel::Info,
        ora_logging::LogOutput::Stdout,
        config.timezone.parse()?,
    ))?;
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?
        .block_on(async move {
            let mut terminate = signal(SignalKind::terminate())?;
            let shutdown = ora_node::Shutdown::default();
            let service = ora_node::serve(config, shutdown.clone());
            tokio::pin!(service);
            tokio::select! {
                result = &mut service => result,
                _ = terminate.recv() => { shutdown.request(); service.await },
            }
        })?;
    Ok(())
}
#[cfg(not(target_os = "linux"))]
fn main() {}
