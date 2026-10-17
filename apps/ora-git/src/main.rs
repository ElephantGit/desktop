//! `ora-git`: asks the Node to fetch from or push to the Git Server for this Agent session.
//!
//! Exit codes and the `ora-git: <code>: <hint>` line on stderr are stable (Node clone ADR D5);
//! progress goes to stderr and the result to stdout.

use ora_git::{ErrorCode, Request, SOCKET_ENV};
use std::process::ExitCode;

/// Runs one request and maps the answer to the process exit code.
fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(output) => {
            print!("{output}");
            ExitCode::SUCCESS
        }
        Err((code, message)) => {
            eprintln!("ora-git: {}: {message}", code.as_str());
            ExitCode::from(u8::try_from(code.exit_code()).unwrap_or(1))
        }
    }
}

/// Validates the command line and sends it on the session's socket.
fn run(args: &[String]) -> Result<String, (ErrorCode, String)> {
    let request = Request::parse(args).map_err(|message| (ErrorCode::Usage, message))?;
    let socket = std::env::var_os(SOCKET_ENV).ok_or_else(|| {
        (
            ErrorCode::NoSession,
            format!("{SOCKET_ENV} is not set; ora-git works only inside an Agent session"),
        )
    })?;
    ora_git::send(
        std::path::Path::new(&socket),
        &request,
        &mut std::io::stderr(),
    )
}
