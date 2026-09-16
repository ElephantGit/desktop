//! Pack install orchestration and ownership-aware uninstall: static preflight, sequential
//! member installs, and ownership-aware member removal.
//!
//! A `kind = "pack"` listing is an orchestration entry, not a package: this module expands one
//! pack installation into member installations that reuse the ordinary single-plugin chain
//! (resolve → release selection → `Installer` download/verify/stage/commit → finalization) with
//! no new on-disk path. Every static problem is found before the first member downloads a byte
//! (extension-pack decision D5/D6): a preflight failure is a typed error with zero side effects,
//! and a member failure mid-run stops the run and is reported inside an `Ok` pack outcome rather
//! than hiding partial success. Uninstall mirrors the same discipline: a plan is computed from
//! the ownership journal and reconciliation first, then executed through the ordinary
//! single-plugin uninstall chain.

use super::PluginApi;
use super::pack_reconcile::PackMemberReconciliation;
use crate::error::{BackendError, ErrorClassification};
use ora_application::Clock;
use ora_contracts::{
    EmptyErrorParams, InstallOutcome, InstallPluginRequest, InstallPluginResponse,
    PackInstallFailure, PackInstalledMember, PackMemberInstallOutcome, PackMemberParams,
    PluginDataDisposition, PublicError, UninstallPluginRequest, UninstallPluginResponse,
};
use ora_db::{PackInstallationMemberRecord, PackInstallationRecord, PackMemberOwnership};
use ora_domain::{PluginId, PluginNamespace};
use ora_logging::ora_info;
use ora_plugin_manager::{
    HostTarget, Installer, PluginContribution, PluginManager, ResolvedReleaseSource, select_release,
};
use ora_plugin_manifest::{PackAgentRef, PluginKind, PluginManifest};
use ora_plugin_registry::RegistryIndex;
use ora_utils::http::{HttpDownload, Progress, ProgressCallback, S3Config};
use std::collections::BTreeSet;
use std::sync::Arc;

/// One applicable pack member with everything its install needs, resolved during preflight.
#[derive(Debug)]
pub(super) struct PackMemberRelease {
    pub(super) plugin_id: PluginId,
    pub(super) manifest: PluginManifest,
    pub(super) release: ResolvedReleaseSource,
}

/// The preflight result: members to install in declaration order plus members already installed.
#[derive(Debug)]
pub(super) struct PackPreflight {
    applicable: Vec<PackMemberRelease>,
    already_installed: Vec<String>,
}

// The accessors exist for in-crate tests; production code in this module reads the fields
// directly because it shares the struct's module.
#[cfg(test)]
impl PackPreflight {
    /// Returns the members the install loop will install, in declaration order.
    pub(super) fn applicable(&self) -> &[PackMemberRelease] {
        &self.applicable
    }

    /// Returns the members mutably so a caller can retarget their download sources.
    pub(super) fn applicable_mut(&mut self) -> &mut Vec<PackMemberRelease> {
        &mut self.applicable
    }

    /// Returns the canonical ids of applicable members that are already installed.
    pub(super) fn already_installed(&self) -> &[String] {
        &self.already_installed
    }
}

/// What one pack install run actually did, consumed by the durable ownership journal (D3-A).
#[derive(Debug, Default)]
pub(super) struct PackRunLedger {
    /// Members this run created: `(canonical id, version that landed)`.
    installed: Vec<(String, String)>,
    /// Applicable members that were already installed and therefore skipped.
    skipped: Vec<String>,
}

/// Why a pack uninstall preserves a member instead of removing it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum PackPreserveReason {
    /// The member was already installed when the pack named it; the pack never created it.
    PreExisting,
    /// The pack created the member, but it has since been independently changed.
    VersionChanged,
}

/// The computed, not-yet-executed ownership-aware uninstall plan for one recorded pack.
///
/// Only removable member ids drive filesystem work; preserved and already-missing members are
/// listed so callers can suspend, resume, and present the full member set without re-deriving
/// the plan.
#[derive(Debug)]
pub(super) struct PackUninstallPlan {
    pack_id: String,
    remove: Vec<String>,
    preserve: Vec<(String, PackPreserveReason)>,
    already_missing: Vec<String>,
}

impl PackUninstallPlan {
    /// Returns the canonical ids of members that will be uninstalled.
    pub(super) fn remove(&self) -> &[String] {
        &self.remove
    }

    /// Returns every member id the journal holds, regardless of disposition.
    pub(super) fn all_member_ids(&self) -> Vec<String> {
        let mut ids = self.remove.clone();
        ids.extend(self.preserve.iter().map(|(member_id, _)| member_id.clone()));
        ids.extend(self.already_missing.iter().cloned());
        ids
    }
}

impl PluginApi {
    /// Installs one `kind = "pack"` listing: preflight the membership, then install every
    /// applicable member in declaration order through the ordinary single-plugin chain.
    ///
    /// The response always carries the pack's own id; the outcome distinguishes installed
    /// members, skipped members, and the first failure.
    pub(super) async fn install_pack(
        &self,
        request: InstallPluginRequest,
        manifest: PluginManifest,
        namespace: PluginNamespace,
        use_proxy: bool,
        s3_config: Option<S3Config>,
        progress: Option<ProgressCallback>,
    ) -> Result<InstallPluginResponse, BackendError> {
        let source = self.owning_registry_source(&namespace).await?;
        let preflight = self.preflight_pack(&manifest, &namespace, &source)?;
        let installer = self.marketplace_installer(use_proxy, s3_config).await?;
        let (outcome, ledger) = self
            .install_members(&namespace, preflight, &installer, progress)
            .await?;
        // The ownership record is written for complete and member-failed runs alike: members the
        // run created become `ManagedByPack`, members the pack merely named keep whatever
        // relationship they already had (or become `PreExisting`). A failed member is absent, so
        // a later pack install that lands it extends the record (decision D7: reinstalling a pack
        // fills in what is missing).
        let pack_id = self.pack_member_id(&namespace, manifest.name().as_str())?;
        self.record_pack_run(&pack_id, source.canonical_url(), &ledger)?;
        ora_info!(plugin_id = %request.plugin_id, outcome = ?outcome, "installed marketplace pack");
        Ok(InstallPluginResponse {
            plugin_id: request.plugin_id,
            outcome,
        })
    }

    /// Runs every static pack check before any member downloads a byte.
    ///
    /// The checks follow the decision's order so the first error is the most actionable one:
    /// duplicate members, self-reference, per-member existence (resolved inside the pack's own
    /// source checkout), nesting, agent filtering, host compatibility, and the already-installed
    /// split. Failure is a typed public error; success carries the resolved per-member state the
    /// install loop consumes without re-reading anything.
    pub(super) fn preflight_pack(
        &self,
        pack: &PluginManifest,
        namespace: &PluginNamespace,
        source: &ora_plugin_registry::RegistrySource,
    ) -> Result<PackPreflight, BackendError> {
        let pack_name = pack.name().as_str();
        let members = pack.pack().ok_or_else(|| {
            BackendError::internal(
                "pack manifest carries no membership",
                std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    pack.name().as_str().to_string(),
                ),
            )
        })?;

        let mut seen = BTreeSet::new();
        let mut declared = Vec::new();
        for member in members.members() {
            let member_id = self.pack_member_id(namespace, member.identifier().as_str())?;
            if !seen.insert(member.identifier().as_str().to_owned()) {
                return Err(self.pack_member_error(PublicError::PackMemberDuplicate, &member_id));
            }
            if member.identifier().as_str() == pack_name {
                return Err(self.pack_member_error(PublicError::PackSelfReference, &member_id));
            }
            // Members resolve inside the pack's own source checkout (decision D2): the member
            // identifier is a bare name, so only the pack's repository can give it an identity.
            let member_manifest =
                RegistryIndex::resolve_manifest(source, &member_id).map_err(|error| {
                    BackendError::internal("failed to resolve pack member manifest", error)
                })?;
            let member_manifest = member_manifest.ok_or_else(|| {
                self.pack_member_error(PublicError::PackMemberNotFound, &member_id)
            })?;
            // V1 forbids nested packs: a member that is itself a pack would turn installation
            // into an unbounded traversal that the decision explicitly leaves to a later ADR.
            if matches!(member_manifest.kind(), PluginKind::Pack) {
                return Err(self.pack_member_error(PublicError::PackMemberNested, &member_id));
            }
            declared.push((member_id, member.agents().to_vec(), member_manifest));
        }

        // Agent filtering is a one-shot listing-time decision (decision D4): a member with agent
        // references belongs to this installation only when some installed agent plugin matches
        // one reference. An unmatched reference is not an error; it only thins the set.
        let installed = PluginManager::discover(&self.home_directory);
        let installed_agents = installed
            .installed_plugins()
            .iter()
            .filter(|plugin| matches!(plugin.contributes, PluginContribution::Agent(_)))
            .map(|plugin| plugin.id.clone())
            .collect::<Vec<_>>();
        let selected = declared
            .into_iter()
            .filter(|(_member_id, agents, _manifest)| {
                agents.is_empty()
                    || agents.iter().any(|reference| {
                        installed_agents.iter().any(|agent_id| match reference {
                            PackAgentRef::Bare(name) => agent_id.name() == name.as_str(),
                            PackAgentRef::Canonical(canonical) => {
                                agent_id.canonical() == canonical.canonical()
                            }
                        })
                    })
            })
            .collect::<Vec<_>>();
        if selected.is_empty() {
            return Err(BackendError::new(
                ErrorClassification::Unprocessable,
                PublicError::PackNoApplicableMembers(PackMemberParams {
                    plugin_id: format!("{namespace}/{}", pack.name().as_str()),
                }),
                "pack has no member that applies to this installation",
            ));
        }

        // Host compatibility is checked here rather than mid-run, so an incompatible member
        // fails the whole pack before the first member lands (decision D6).
        let host_target = ora_plugin_registry::current_host_target();
        let host = HostTarget::from_option(host_target.as_ref());
        let mut already_installed = Vec::new();
        let mut applicable = Vec::new();
        for (member_id, _agents, member_manifest) in selected {
            let release = select_release(&member_manifest, host).map_err(|error| {
                self.map_install_error("failed to select pack member release", error)
            })?;
            if installed
                .installed_plugins()
                .iter()
                .any(|plugin| plugin.id.canonical() == member_id.canonical())
            {
                already_installed.push(member_id.canonical());
            } else {
                applicable.push(PackMemberRelease {
                    plugin_id: member_id,
                    manifest: member_manifest,
                    release,
                });
            }
        }
        Ok(PackPreflight {
            applicable,
            already_installed,
        })
    }

    /// Installs the applicable members in declaration order, skipping members that were installed
    /// between preflight and their turn, and reporting the first member failure inside the
    /// returned outcome instead of rolling back members that already landed.
    pub(super) async fn install_members<D>(
        &self,
        namespace: &PluginNamespace,
        preflight: PackPreflight,
        installer: &Installer<D>,
        progress: Option<ProgressCallback>,
    ) -> Result<(InstallOutcome, PackRunLedger), BackendError>
    where
        D: HttpDownload,
    {
        let member_total = preflight.applicable.len();
        // The pack forwards one aggregated progress stream under its own id: each member's inner
        // byte progress is scaled into its share of the member count.
        let member_scale: u64 = 1_000_000;
        let mut members = Vec::new();
        let mut skipped = preflight.already_installed;
        // The ledger records what this run actually did, for the durable ownership journal.
        let mut ledger = PackRunLedger::default();
        for (index, member) in preflight.applicable.into_iter().enumerate() {
            // Re-checked at install time: a concurrent install between preflight and this member's
            // turn makes it a skip rather than a duplicate directory (invariant 5).
            if self.member_is_installed(&member.plugin_id) {
                skipped.push(member.plugin_id.canonical());
                continue;
            }
            let member_version = member.manifest.version().to_string();
            let member_progress: Option<ProgressCallback> = progress.as_ref().map(|progress| {
                let progress = Arc::clone(progress);
                Arc::new(move |inner: Progress| {
                    let fraction = inner
                        .total
                        .map(|total| inner.bytes as f64 / total as f64)
                        .unwrap_or(0.0);
                    progress(Progress {
                        bytes: ((index as f64 + fraction) * member_scale as f64) as u64,
                        total: Some(member_total as u64 * member_scale),
                    });
                }) as ProgressCallback
            });
            let install_result = match member_progress {
                Some(member_progress) => {
                    installer
                        .install_with_progress(
                            &member.manifest,
                            namespace,
                            member.release,
                            &self.home_directory,
                            member_progress,
                        )
                        .await
                }
                None => {
                    installer
                        .install(
                            &member.manifest,
                            namespace,
                            member.release,
                            &self.home_directory,
                        )
                        .await
                }
            };
            if let Err(error) = install_result {
                let mapped = self.map_install_error("failed to install pack member", error);
                ora_info!(
                    plugin_id = %member.plugin_id.canonical(),
                    error = %mapped,
                    "pack member installation failed; remaining members are not attempted"
                );
                return Ok((
                    InstallOutcome::PackInstalled {
                        members,
                        skipped,
                        failed: Some(PackInstallFailure {
                            plugin_id: member.plugin_id.canonical(),
                            error_code: mapped.public_error().code().to_owned(),
                        }),
                    },
                    ledger,
                ));
            }
            // Finalize exactly like a single-plugin install so Skills, the installed snapshot,
            // and MCP desired state see the member immediately; a finalization failure is that
            // member's failure and stops the run on the same terms as a download failure.
            let member_outcome = match self
                .finalize_new_install(&member.plugin_id.canonical())
                .await
            {
                Ok(outcome) => outcome,
                Err(error) => {
                    return Ok((
                        InstallOutcome::PackInstalled {
                            members,
                            skipped,
                            failed: Some(PackInstallFailure {
                                plugin_id: member.plugin_id.canonical(),
                                error_code: error.public_error().code().to_owned(),
                            }),
                        },
                        ledger,
                    ));
                }
            };
            let member_outcome = match member_outcome {
                InstallOutcome::Installed => PackMemberInstallOutcome::Installed,
                InstallOutcome::InstalledWithCommandConflict { conflict_plugin_id } => {
                    PackMemberInstallOutcome::InstalledWithCommandConflict { conflict_plugin_id }
                }
                // Members run the single-plugin chain, which cannot produce a pack outcome.
                InstallOutcome::PackInstalled { .. } => {
                    unreachable!("member finalization cannot produce a pack outcome")
                }
            };
            // This run created the member, so the durable relationship records it as
            // pack-managed (D3-A).
            ledger
                .installed
                .push((member.plugin_id.canonical(), member_version));
            members.push(PackInstalledMember {
                plugin_id: member.plugin_id.canonical(),
                outcome: member_outcome,
            });
        }
        ledger.skipped = skipped.clone();
        Ok((
            InstallOutcome::PackInstalled {
                members,
                skipped,
                failed: None,
            },
            ledger,
        ))
    }

    /// Assembles the canonical member id a bare pack member identifier resolves to inside the
    /// pack's own namespace.
    fn pack_member_id(
        &self,
        namespace: &PluginNamespace,
        identifier: &str,
    ) -> Result<PluginId, BackendError> {
        PluginId::new(namespace.clone(), identifier).map_err(|error| {
            BackendError::internal(
                "pack member identifier is not representable as a plugin id",
                error,
            )
        })
    }

    /// Builds the typed preflight failure for one member.
    fn pack_member_error(
        &self,
        public_error: impl FnOnce(PackMemberParams) -> PublicError,
        member_id: &PluginId,
    ) -> BackendError {
        BackendError::new(
            ErrorClassification::Unprocessable,
            public_error(PackMemberParams {
                plugin_id: member_id.canonical(),
            }),
            format!("pack member {} failed preflight", member_id.canonical()),
        )
    }

    /// Returns whether `member_id` is currently installed, by rescanning the package tree.
    ///
    /// Discovery is the authority for installation state; the cached lifecycle snapshot can lag a
    /// concurrent operation, and a stale skip reads as an install error rather than a silent
    /// downgrade.
    fn member_is_installed(&self, member_id: &PluginId) -> bool {
        PluginManager::discover(&self.home_directory)
            .installed_plugins()
            .iter()
            .any(|plugin| plugin.id.canonical() == member_id.canonical())
    }

    /// Persists what one pack install run did into the durable ownership journal.
    ///
    /// Members the run created are recorded `ManagedByPack` at the version that landed. Skipped
    /// members keep the relationship they already have; one the pack never installed is recorded
    /// `PreExisting` at its current version. A member that failed mid-run is left absent so a
    /// later pack install that lands it extends the record.
    pub(super) fn record_pack_run(
        &self,
        pack_id: &PluginId,
        source_url: &str,
        ledger: &PackRunLedger,
    ) -> Result<(), BackendError> {
        let canonical = pack_id.canonical();
        let now = self.clock.now_timestamp_millis();
        self.pack_installations
            .upsert_pack(&canonical, source_url, now)
            .map_err(|error| {
                BackendError::internal("failed to record the pack installation", error)
            })?;
        for (member_id, version) in &ledger.installed {
            self.pack_installations
                .upsert_member(
                    &canonical,
                    &PackInstallationMemberRecord {
                        member_id: member_id.clone(),
                        version_at_install: version.clone(),
                        ownership: PackMemberOwnership::ManagedByPack,
                    },
                    now,
                )
                .map_err(|error| {
                    BackendError::internal("failed to record the pack member ownership", error)
                })?;
        }
        for member_id in &ledger.skipped {
            // A member the pack named but never touched keeps the relationship it already has;
            // only a first-time skip becomes a `PreExisting` relationship.
            if self
                .pack_installations
                .load_member(&canonical, member_id)
                .map_err(|error| {
                    BackendError::internal("failed to load the pack member relationship", error)
                })?
                .is_some()
            {
                continue;
            }
            let version = PluginManager::discover(&self.home_directory)
                .installed_plugins()
                .iter()
                .find(|plugin| plugin.id.canonical() == *member_id)
                .map(|plugin| plugin.version.to_string())
                .ok_or_else(|| {
                    BackendError::internal(
                        "skipped pack member disappeared before its ownership was recorded",
                        std::io::Error::new(std::io::ErrorKind::NotFound, member_id.clone()),
                    )
                })?;
            self.pack_installations
                .upsert_member(
                    &canonical,
                    &PackInstallationMemberRecord {
                        member_id: member_id.clone(),
                        version_at_install: version,
                        ownership: PackMemberOwnership::PreExisting,
                    },
                    now,
                )
                .map_err(|error| {
                    BackendError::internal("failed to record the pack member relationship", error)
                })?;
        }
        Ok(())
    }

    /// Loads one recorded pack installation with its member relationships.
    ///
    /// This is the read side of the ownership journal that restart reconciliation (D3-B) and
    /// pack uninstall (D3-C) build on.
    pub(super) fn pack_installation(
        &self,
        pack_id: &str,
    ) -> Result<Option<PackInstallationRecord>, BackendError> {
        self.pack_installations.load(pack_id).map_err(|error| {
            BackendError::internal("failed to load pack installation record", error)
        })
    }

    /// Computes the ownership-aware uninstall plan for one recorded pack, or `None` when the id
    /// carries no ownership journal (an ordinary single-plugin uninstall then applies).
    ///
    /// Only members the pack created at the version it recorded are removable; everything else
    /// is preserved with the reason, so a pack uninstall can never touch user assets.
    pub(super) fn pack_uninstall_plan(
        &self,
        pack_id: &str,
    ) -> Result<Option<PackUninstallPlan>, BackendError> {
        let Some(reconciliation) = self.reconcile_pack_installation(pack_id)? else {
            return Ok(None);
        };
        let mut plan = PackUninstallPlan {
            pack_id: pack_id.to_owned(),
            remove: Vec::new(),
            preserve: Vec::new(),
            already_missing: Vec::new(),
        };
        for member in reconciliation.members() {
            match member {
                PackMemberReconciliation::ExpectedAndPresent {
                    member_id,
                    ownership: PackMemberOwnership::ManagedByPack,
                    ..
                } => plan.remove.push(member_id.clone()),
                PackMemberReconciliation::ExpectedAndPresent {
                    member_id,
                    ownership: PackMemberOwnership::PreExisting,
                    ..
                } => plan
                    .preserve
                    .push((member_id.clone(), PackPreserveReason::PreExisting)),
                // A version change means the user took over the member: the classification is
                // reported, the member is kept, and its ownership is never re-derived.
                PackMemberReconciliation::VersionChanged { member_id, .. } => plan
                    .preserve
                    .push((member_id.clone(), PackPreserveReason::VersionChanged)),
                PackMemberReconciliation::Missing {
                    member_id,
                    ownership: PackMemberOwnership::ManagedByPack,
                    ..
                } => plan.already_missing.push(member_id.clone()),
                PackMemberReconciliation::Missing {
                    member_id,
                    ownership: PackMemberOwnership::PreExisting,
                    ..
                } => plan
                    .preserve
                    .push((member_id.clone(), PackPreserveReason::PreExisting)),
            }
        }
        Ok(Some(plan))
    }

    /// Executes an ownership-aware pack uninstall: removable members go through the ordinary
    /// single-plugin uninstall chain, preserved members keep their packages, and the ownership
    /// journal releases one relationship at a time so a mid-run failure leaves a retryable state.
    ///
    /// The pack root record is deleted only after every journal relationship has been released;
    /// a member uninstall that fails keeps its journal row and stops the run, and a retry
    /// re-plans from the surviving journal.
    pub(super) async fn uninstall_pack(
        &self,
        plan: PackUninstallPlan,
        data_disposition: PluginDataDisposition,
    ) -> Result<UninstallPluginResponse, BackendError> {
        let pack_id = plan.pack_id.clone();
        for member_id in plan.remove {
            let result = self
                .uninstall(UninstallPluginRequest {
                    plugin_id: member_id.clone(),
                    data_disposition,
                })
                .await;
            if let Err(error) = result {
                // The member keeps its journal row: the next uninstall re-plans and continues
                // from exactly this member.
                return Err(BackendError::new(
                    error.classification(),
                    error.public_error().clone(),
                    format!("pack member {member_id} could not be uninstalled: {error}"),
                ));
            }
            self.pack_installations
                .remove_member(&pack_id, &member_id)
                .map_err(|error| {
                    BackendError::internal(
                        "failed to release the removed member relationship",
                        error,
                    )
                })?;
        }
        // Preserved members and already-absent members leave the journal deliberately: the pack
        // uninstall dissolves the relationship without touching their packages.
        for (member_id, _reason) in plan.preserve {
            self.pack_installations
                .remove_member(&pack_id, &member_id)
                .map_err(|error| {
                    BackendError::internal(
                        "failed to release the preserved member relationship",
                        error,
                    )
                })?;
        }
        for member_id in plan.already_missing {
            self.pack_installations
                .remove_member(&pack_id, &member_id)
                .map_err(|error| {
                    BackendError::internal(
                        "failed to release the already-missing member relationship",
                        error,
                    )
                })?;
        }
        // The root record goes only when no relationship remains; a failed member keeps the
        // journal alive for the retry.
        let remaining = self
            .pack_installations
            .load(&pack_id)
            .map_err(|error| BackendError::internal("failed to load the pack journal", error))?;
        if remaining.is_none_or(|record| record.members.is_empty()) {
            self.pack_installations.remove(&pack_id).map_err(|error| {
                BackendError::internal("failed to remove the pack journal", error)
            })?;
        }
        ora_info!(plugin_id = %pack_id, "uninstalled marketplace pack");
        Ok(UninstallPluginResponse { plugin_id: pack_id })
    }

    /// Resolves the one marketplace source whose namespace owns `namespace`.
    ///
    /// Members always resolve inside the pack's own source (decision D2), so the orchestrator
    /// needs that source's checkout and proxy policy rather than the whole source list.
    async fn owning_registry_source(
        &self,
        namespace: &PluginNamespace,
    ) -> Result<ora_plugin_registry::RegistrySource, BackendError> {
        let proxy_settings = self.settings.network_proxy_settings().await?;
        let registry_sources = self.prepared_registry_sources(proxy_settings)?;
        registry_sources
            .into_iter()
            .find(|(source, _use_proxy, _s3_config)| {
                source.namespace().as_str() == namespace.as_str()
            })
            .map(|(source, _use_proxy, _s3_config)| source)
            .ok_or_else(|| {
                BackendError::new(
                    ErrorClassification::NotFound,
                    PublicError::PluginNotFound(EmptyErrorParams {}),
                    format!("no marketplace source owns the namespace {namespace}"),
                )
            })
    }
}
