use std::process::ExitCode;

/// Keeps argument parsing outside the runtime's privileged deployment checks.
fn main() -> ExitCode {
    let arguments: Vec<_> = std::env::args_os().skip(1).collect();
    if arguments.len() != 2 || arguments[0] != "--check" {
        eprintln!("usage: ora-process-helper --check <absolute-config-path>");
        return ExitCode::FAILURE;
    }
    #[cfg(target_os = "linux")]
    {
        match ora_process_runtime::check_linux_helper_deployment(std::path::Path::new(
            &arguments[1],
        )) {
            Ok(()) => {
                println!("deployment prerequisites checked; workload launch is not available");
                ExitCode::SUCCESS
            }
            Err(error) => {
                eprintln!("ora-process-helper: {error}");
                ExitCode::FAILURE
            }
        }
    }
    #[cfg(not(target_os = "linux"))]
    {
        eprintln!("ora-process-helper deployment is supported only on Linux");
        ExitCode::FAILURE
    }
}
