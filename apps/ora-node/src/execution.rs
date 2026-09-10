use crate::resources::failure;
use crate::{
    Clock, Command, DirectoryState, Error, Node, NodeState, Observation, WorktreeGit, WriteGuard,
};
use ora_node_db::{Execution, Progress, ResourceState, Stage, Target};
use ora_node_protocol::*;

impl<G: WorktreeGit, W: WriteGuard, C: Clock> Node<G, W, C> {
    /// Deduplicates before touching current configuration or Git; new mutations require durable acceptance.
    pub fn submit(&mut self, command: Command) -> Result<ExecutionStatus, Error> {
        command.validate()?;
        if command.spec().node_id != self.identity.node_id {
            return Err(ora_node_db::Error::NodeMismatch.into());
        }
        if let Some(record) = self.database.existing(&command)? {
            return Ok(ExecutionStatus {
                node: self.identity.clone(),
                state: record.progress.state(),
            });
        }
        if self.state == NodeState::RecoveryPending {
            return Err(Error::RecoveryPending);
        }
        let target = match self.resolve(&command) {
            Ok(target) => target,
            Err(reason) => {
                let result = self.failed(&command, reason);
                self.database.reject(&command, result.clone())?;
                return Ok(ExecutionStatus {
                    node: self.identity.clone(),
                    state: ExecutionState::Completed(result),
                });
            }
        };
        let record = match self.database.accept(&command, &target) {
            Ok(record) => record,
            Err(ora_node_db::Error::ResourceConflict) => {
                let result = self.failed(
                    &command,
                    failure(
                        WorktreeFailureCode::WorktreeConflict,
                        "resource is reserved or owned by another execution",
                    ),
                );
                self.database.reject(&command, result.clone())?;
                return Ok(ExecutionStatus {
                    node: self.identity.clone(),
                    state: ExecutionState::Completed(result),
                });
            }
            Err(error) => return Err(error.into()),
        };
        // Any error from this point must leave recovery gating in place, including disk failures.
        self.state = NodeState::RecoveryPending;
        self.drive(record)?;
        if self.database.recoverable()?.is_empty() {
            self.state = NodeState::Ready;
        }
        let record = self
            .database
            .existing(&command)?
            .ok_or(ora_node_db::Error::InvalidTransition)?;
        Ok(ExecutionStatus {
            node: self.identity.clone(),
            state: record.progress.state(),
        })
    }

    /// Reconciles every incomplete execution under its original identity and frozen target.
    /// Unknown evidence remains retryable and gates new mutations while queries and replay stay available.
    pub fn recover(&mut self) -> Result<NodeState, Error> {
        self.state = NodeState::RecoveryPending;
        for record in self.database.recoverable()? {
            self.drive(record)?;
        }
        if self.database.recoverable()?.is_empty() {
            self.state = NodeState::Ready;
        }
        Ok(self.state)
    }

    /// Dispatches the same fact-driven state machine for initial execution and restart recovery.
    fn drive(&mut self, record: Execution) -> Result<(), Error> {
        let target = record
            .target
            .clone()
            .ok_or(ora_node_db::Error::InvalidTransition)?;
        let stage = match record.progress {
            Progress::Accepted => match record.command {
                Command::Ensure(_) => Stage::Create,
                Command::Remove(_) => Stage::RemoveWorktree,
            },
            Progress::Running { stage, .. } | Progress::Unknown { stage, .. } => stage,
            Progress::Completed { .. } => return Ok(()),
        };
        let observation = match self
            .verify(&record.command, &target)
            .and_then(|()| self.git.observe(&target))
        {
            Ok(observation) => observation,
            Err(reason) => return self.unknown(&record, stage, reason),
        };
        match &record.command {
            Command::Ensure(_) => self.ensure(record, &target, observation),
            Command::Remove(_) => self.remove(record, &target, observation),
        }
    }

    /// Completes creation only with matching registration, branch, checkout and frozen baseline evidence.
    fn ensure(
        &mut self,
        record: Execution,
        target: &Target,
        observed: Observation,
    ) -> Result<(), Error> {
        if let Some(checkout) = &observed.checkout
            && !observed.branch_elsewhere
            && observed.branch.as_ref() == Some(&target.base_commit)
            && checkout.head == target.base_commit
            && checkout.branch == target.branch
            && checkout.path == target.path
            && observed.directory == DirectoryState::Nonempty
        {
            let result =
                WorktreeExecutionResult::Ready(WorktreeReady {
                    node: self.identity.clone(),
                    workspace_id: record.command.spec().workspace_id.clone(),
                    worktree_id: record.command.spec().worktree_id.clone(),
                    facts: WorktreeFacts {
                        path: NodePath::new(checkout.path.to_str().ok_or_else(|| {
                            Error::Configuration("non-UTF-8 worktree path".into())
                        })?),
                        branch: checkout.branch.clone(),
                        base_commit: target.base_commit.clone(),
                    },
                });
            return Ok(self.database.complete(&record, result)?);
        }
        if !observed.absent() {
            return self.unknown(
                &record,
                Stage::Create,
                failure(
                    WorktreeFailureCode::ResultUnknown,
                    "creation facts are incomplete or differ from the frozen baseline",
                ),
            );
        }
        let running = self.running(&record, Stage::Create)?;
        let mutation = self
            .verify(&record.command, target)
            .and_then(|()| self.git.create(target));
        let after = match self
            .verify(&record.command, target)
            .and_then(|()| self.git.observe(target))
        {
            Ok(after) => after,
            Err(reason) => return self.unknown(&running, Stage::Create, reason),
        };
        if after.absent() {
            let reason = mutation.err().unwrap_or_else(|| {
                failure(
                    WorktreeFailureCode::OperationFailed,
                    "Git add left no worktree or branch",
                )
            });
            return Ok(self
                .database
                .complete(&running, self.failed(&record.command, reason))?);
        }
        // Re-enter only the observation path: non-absent evidence cannot issue another add.
        self.ensure(running, target, after)
    }

    /// Removes checkout, residual empty directory and owned branch in separately persisted stages.
    fn remove(
        &mut self,
        mut record: Execution,
        target: &Target,
        mut observed: Observation,
    ) -> Result<(), Error> {
        let initially_absent = observed.absent();
        let resource = self
            .database
            .resource(&record.command.spec().worktree_id)?
            .ok_or(ora_node_db::Error::ResourceConflict)?;
        if resource.state == ResourceState::Removed && !initially_absent {
            return self.unknown(
                &record,
                Stage::RemoveWorktree,
                failure(
                    WorktreeFailureCode::WorktreeConflict,
                    "resources appeared after their ownership was retired",
                ),
            );
        }
        if observed.branch_elsewhere
            || (observed.checkout.is_none() && observed.directory == DirectoryState::Nonempty)
        {
            return self.unknown(
                &record,
                Stage::RemoveWorktree,
                failure(
                    WorktreeFailureCode::WorktreeConflict,
                    "branch is occupied elsewhere or unregistered residual files remain",
                ),
            );
        }
        if observed.checkout.is_some() {
            record = self.running(&record, Stage::RemoveWorktree)?;
            if let Err(reason) = self
                .verify(&record.command, target)
                .and_then(|()| self.git.remove_worktree(target))
            {
                return self.unknown(&record, Stage::RemoveWorktree, reason);
            }
            observed = match self
                .verify(&record.command, target)
                .and_then(|()| self.git.observe(target))
            {
                Ok(after) => after,
                Err(reason) => return self.unknown(&record, Stage::RemoveWorktree, reason),
            };
        }
        if observed.checkout.is_some()
            || observed.directory == DirectoryState::Nonempty
            || observed.branch_elsewhere
        {
            return self.unknown(
                &record,
                Stage::RemoveWorktree,
                failure(
                    WorktreeFailureCode::ResultUnknown,
                    "checkout removal could not be proven",
                ),
            );
        }
        if observed.directory == DirectoryState::Empty {
            record = self.running(&record, Stage::RemoveWorktree)?;
            if let Err(reason) = self
                .verify(&record.command, target)
                .and_then(|()| self.git.remove_empty_directory(target))
            {
                return self.unknown(&record, Stage::RemoveWorktree, reason);
            }
        }
        if observed.branch.is_some() {
            record = self.running(&record, Stage::RemoveBranch)?;
            // Re-observe before deleting a branch after a separate filesystem mutation.
            let check = self
                .verify(&record.command, target)
                .and_then(|()| self.git.observe(target));
            match check {
                Ok(facts)
                    if !facts.branch_elsewhere
                        && facts.checkout.is_none()
                        && facts.directory == DirectoryState::Absent => {}
                Ok(_) => {
                    return self.unknown(
                        &record,
                        Stage::RemoveBranch,
                        failure(
                            WorktreeFailureCode::WorktreeConflict,
                            "ownership facts changed before branch cleanup",
                        ),
                    );
                }
                Err(reason) => return self.unknown(&record, Stage::RemoveBranch, reason),
            }
            if let Err(reason) = self.git.remove_branch(target) {
                return self.unknown(&record, Stage::RemoveBranch, reason);
            }
        }
        match self
            .verify(&record.command, target)
            .and_then(|()| self.git.observe(target))
        {
            Ok(after) if after.absent() => {
                let result = WorktreeExecutionResult::Removed(WorktreeRemoved {
                    node: self.identity.clone(),
                    workspace_id: record.command.spec().workspace_id.clone(),
                    worktree_id: record.command.spec().worktree_id.clone(),
                    outcome: if initially_absent {
                        WorktreeRemovalOutcome::AlreadyAbsent
                    } else {
                        WorktreeRemovalOutcome::Removed
                    },
                });
                Ok(self.database.complete(&record, result)?)
            }
            Ok(_) => self.unknown(
                &record,
                Stage::RemoveBranch,
                failure(
                    WorktreeFailureCode::ResultUnknown,
                    "not all cleanup targets are absent",
                ),
            ),
            Err(reason) => self.unknown(&record, Stage::RemoveBranch, reason),
        }
    }

    /// Persists mutation intent and the acting incarnation before calling Git.
    fn running(&mut self, record: &Execution, stage: Stage) -> Result<Execution, Error> {
        Ok(self.database.advance(
            record,
            Progress::Running {
                stage,
                observer: self.identity.clone(),
                observed_at: self.clock.now(),
            },
        )?)
    }

    /// Keeps inconclusive results recoverable instead of sealing them as terminal failures.
    fn unknown(
        &mut self,
        record: &Execution,
        stage: Stage,
        reason: WorktreeFailure,
    ) -> Result<(), Error> {
        self.database.advance(
            record,
            Progress::Unknown {
                stage,
                observer: self.identity.clone(),
                observed_at: self.clock.now(),
                diagnostic: reason.message,
            },
        )?;
        Ok(())
    }

    /// Maps only definitive failures to the operation-specific protocol result.
    fn failed(&self, command: &Command, failure: WorktreeFailure) -> WorktreeExecutionResult {
        match command {
            Command::Ensure(_) => WorktreeExecutionResult::Failed(WorktreeFailed {
                node: self.identity.clone(),
                workspace_id: command.spec().workspace_id.clone(),
                worktree_id: command.spec().worktree_id.clone(),
                failure,
            }),
            Command::Remove(_) => WorktreeExecutionResult::RemovalFailed(WorktreeRemovalFailed {
                node: self.identity.clone(),
                workspace_id: command.spec().workspace_id.clone(),
                worktree_id: command.spec().worktree_id.clone(),
                failure,
            }),
        }
    }
}
