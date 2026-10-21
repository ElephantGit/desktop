//! Revision delivery's Git runs through the host like clone's, as separate guarded Runs.
//!
//! Restore Git records original identities in the session journal before admission.
//! Delivery Git only reads the checkout, writes scratch files in its Git directory and moves an
//! `refs/ora/revisions/` ref, so its Runs are not journaled as business mutations: a delivery
//! interrupted before freezing prepares again from scratch, and the guardian ends a Run whose
//! Node exited. Each Run gets its own Scope, closed before the next command starts.
use super::{CloneHost, RunAs, transport};
use crate::revision::RestoreGitRunner;
use gitlancer::{GitCommand, GitExecError, GitOutput, GitRunner};
use ora_process_client::ProcessHost;
use ora_process_protocol::*;
use std::time::{Duration, Instant};

/// A restore caps execution only; delivery and cleanup keep their independent configured budgets.
enum Budget {
    Command,
    Restore(Instant),
}

/// The same host Runs as the Node identity, for Git Server access from transport repositories;
/// it borrows or owns its host side.
pub(crate) struct AsNode<H>(pub(crate) H);

impl<H: std::borrow::Borrow<CloneHost>> GitRunner for AsNode<H> {
    /// Captures both streams up to the guardian limit.
    fn run(&self, command: &GitCommand) -> Result<GitOutput, GitExecError> {
        self.run_bounded(command, GUARDIAN_CAPTURE_LIMIT, GUARDIAN_CAPTURE_LIMIT)
    }

    /// Runs one command without the workload isolation, in its own closed Scope.
    fn run_bounded(
        &self,
        command: &GitCommand,
        stdout: usize,
        stderr: usize,
    ) -> Result<GitOutput, GitExecError> {
        self.0
            .borrow()
            .run_scoped(command, stdout, stderr, RunAs::Node, Budget::Command)
    }
}

impl GitRunner for CloneHost {
    /// Captures both streams up to the guardian limit.
    fn run(&self, command: &GitCommand) -> Result<GitOutput, GitExecError> {
        self.run_bounded(command, GUARDIAN_CAPTURE_LIMIT, GUARDIAN_CAPTURE_LIMIT)
    }

    /// Runs one command in a fresh Scope and closes the Scope before returning, so no delivery
    /// Git can still be writing when the next step reads the result.
    fn run_bounded(
        &self,
        command: &GitCommand,
        stdout: usize,
        stderr: usize,
    ) -> Result<GitOutput, GitExecError> {
        self.run_scoped(command, stdout, stderr, RunAs::Workload, Budget::Command)
    }
}

impl RestoreGitRunner for CloneHost {
    /// The restore mutex provides exclusive ownership while binding the command context.
    fn bind_restore(
        &mut self,
        execution: &ora_node_protocol::ExecutionId,
        cancellation: crate::session::RestoreCancel,
    ) {
        self.restore_context = self
            .restore_stops
            .clone()
            .map(|stops| super::RestoreContext {
                stops,
                execution: execution.clone(),
                cancellation,
            });
    }

    /// An admitted command finishes under its budget; cancellation fences the next side effect.
    fn check_admission(&self) -> Result<(), GitExecError> {
        if self.shutdown.requested()
            || self.restore_context.as_ref().is_some_and(|context| {
                context.cancellation.requested()
                    || !context
                        .stops
                        .journal
                        .restore_stop_requested(&context.execution)
                        .is_ok_and(|requested| !requested)
            })
        {
            Err(GitExecError::OutputReadFailed {
                stream: "Revision restore",
                source: std::io::Error::other("restore cancellation requested"),
            })
        } else {
            Ok(())
        }
    }

    /// Caps the host exchange as well as process polling without clipping Scope cleanup.
    fn run_bounded_before(
        &self,
        command: &GitCommand,
        stdout: usize,
        stderr: usize,
        deadline: Instant,
    ) -> Result<GitOutput, GitExecError> {
        self.run_scoped(
            command,
            stdout,
            stderr,
            RunAs::Workload,
            Budget::Restore(deadline),
        )
    }
}

impl CloneHost {
    /// Runs one command as `identity` in a fresh Scope, closed before returning.
    fn run_scoped(
        &self,
        command: &GitCommand,
        stdout: usize,
        stderr: usize,
        identity: RunAs,
        budget: Budget,
    ) -> Result<GitOutput, GitExecError> {
        let fail = |source| GitExecError::OutputReadFailed {
            stream: "delivery Git",
            source,
        };
        if self.shutdown.requested()
            || self.restore_context.as_ref().is_some_and(|context| {
                context.cancellation.requested()
                    || !context
                        .stops
                        .journal
                        .restore_stop_requested(&context.execution)
                        .is_ok_and(|requested| !requested)
            })
        {
            return Err(fail(std::io::Error::other("Node is stopping")));
        }
        if let Budget::Restore(deadline) = budget
            && Instant::now() >= deadline
        {
            return Err(fail(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "restore execution deadline expired",
            )));
        }
        let client = ProcessHost::new(self.config.host_directory.clone(), self.config.expected_uid);
        let intent = HostRunIntent {
            scope: ScopeId::new(),
            run: RunId::new(),
            spec: self.spec_as(
                command,
                OutputPolicy::Capture {
                    stdout_limit: stdout.min(GUARDIAN_CAPTURE_LIMIT),
                    stderr_limit: stderr.min(GUARDIAN_CAPTURE_LIMIT),
                },
                identity,
            ),
            host_disconnect: GuardianHostDisconnect::KeepRunning,
        };

        let command_budget = self.config.budget_for(command);
        if let Some(context) = &self.restore_context {
            context
                .stops
                .journal
                .record_restore_attempt(
                    &ora_node_db::ProcessAttempt {
                        execution: context.execution.clone(),
                        host_directory: self.config.host_directory.clone(),
                        expected_uid: self.config.expected_uid,
                        intent: intent.clone(),
                    },
                    ora_node_protocol::StopContainment::BestEffort,
                )
                .map_err(|error| fail(std::io::Error::other(error)))?;
        }
        let result = match budget {
            Budget::Command => self.runtime.block_on(transport::execute(
                &client,
                &intent,
                command_budget,
                &self.config,
                &self.shutdown,
            )),
            Budget::Restore(deadline) => self.runtime.block_on(transport::execute_before(
                &client,
                &intent,
                command_budget,
                &self.config,
                &super::Shutdown::default(),
                deadline,
            )),
        };
        if let Some(context) = &self.restore_context {
            if !self
                .runtime
                .block_on(
                    context
                        .stops
                        .poll(&context.execution, super::StopCheck::WithinBound),
                )
                .map_err(fail)?
            {
                return Err(fail(std::io::Error::other("restore stop is unconfirmed")));
            }
        } else {
            self.runtime
                .block_on(transport::close(
                    &client,
                    intent.scope,
                    Duration::from_millis(self.config.cleanup_timeout_ms),
                ))
                .map_err(fail)?;
        }
        let output = result.map_err(fail)?;
        if output.code == Some(0) {
            Ok(output)
        } else {
            Err(GitExecError::NonZeroExit {
                code: output.code,
                args: command.args.clone(),
                stdout: output.stdout,
                stderr: output.stderr,
            })
        }
    }
}
