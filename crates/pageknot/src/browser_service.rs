use std::sync::Arc;
use std::time::Duration;

use camino::Utf8PathBuf;
use pageknot_chromium::{
    CdpClient, ChromiumLaunchOptions, ChromiumPage, ChromiumProcess,
    MANAGED_BROWSER_CATALOG_VERSION, ManagedBrowserManager, probe_collector_handshake,
    resolve_remote_endpoint,
};
use pageknot_model::{
    BrowserAction, BrowserCandidate, BrowserCandidateState, BrowserDoctorReport,
    BrowserEnvironment, BrowserInfo, BrowserInstallRequest, BrowserOperationResult, BrowserSource,
    BrowserSpec, CapabilityCheck, CaptureId, ConfigProvenance, EffectiveConfigValue, ErrorStage,
    ManagedBrowserState, NetworkPolicySummary, OutputCapability, PageKnotError, RecoveryAction,
    Result,
};
use pageknot_protocol::{
    COLLECTOR_PROTOCOL_VERSION, COLLECTOR_PROTOCOL_VERSION_STRING, CollectorHandshake,
    negotiate_protocol,
};

use crate::runtime::{RuntimeState, probe_remote_browser};

const COLLECTOR_PROBE_TIMEOUT: Duration = Duration::from_secs(10);
const COLLECTOR_PROBE_CHUNK_BYTES: u64 = 64 * 1024;

#[derive(Clone, Debug)]
/// Discovers, installs, diagnoses, and closes Chromium browser instances.
pub struct BrowserService {
    state: Arc<RuntimeState>,
}

impl BrowserService {
    pub(crate) const fn new(state: Arc<RuntimeState>) -> Self {
        Self { state }
    }

    /// Resolves the configured browser and starts it when needed.
    pub async fn ensure(&self) -> Result<BrowserInfo> {
        self.state
            .ensure_browser(self.state.default_browser())
            .await
    }

    /// Installs a managed browser revision after archive digest verification.
    pub async fn install(&self, request: BrowserInstallRequest) -> Result<BrowserInfo> {
        self.state.ensure_open()?;
        let cache_dir = request
            .cache_dir
            .map(pageknot_model::PortablePath::into_utf8_path_buf)
            .unwrap_or_else(|| self.state.cache_dir.clone());
        ManagedBrowserManager::new(cache_dir)
            .install(request.revision.as_deref())
            .await
    }

    /// Installs a managed browser and returns the stable browser-operation
    /// record used by machine-facing clients.
    pub async fn install_operation(
        &self,
        request: BrowserInstallRequest,
    ) -> Result<BrowserOperationResult> {
        let browser = self.install(request).await?;
        Ok(BrowserOperationResult {
            schema_version: pageknot_model::PUBLIC_SCHEMA_VERSION,
            action: BrowserAction::Install,
            revision: browser.revision.clone(),
            browser: Some(browser),
            cache_dir: self.state.cache_dir.clone().into(),
            candidates: Vec::new(),
        })
    }

    /// Lists compatible, selected, shadowed, and incompatible browser
    /// candidates.
    pub async fn list(&self) -> Result<Vec<BrowserCandidate>> {
        self.state.ensure_open()?;
        let mut discovery = self.state.discovery.discover().await;
        let manager = ManagedBrowserManager::new(self.state.cache_dir.clone());
        for candidate in &mut discovery.candidates {
            if candidate.browser.source == BrowserSource::Managed
                && let Some(revision) = candidate.browser.revision.as_deref()
            {
                candidate.active_leases = manager.active_leases(revision).await?;
            }
        }
        apply_active_leases(
            &mut discovery.candidates,
            self.state.active_browser_snapshot().await,
        );
        Ok(discovery.candidates)
    }

    /// Removes an installed managed browser revision.
    ///
    /// Active leases block removal. `force` permits removal of a selected
    /// revision when another compatible browser remains available.
    pub async fn remove(&self, revision: &str, force: bool) -> Result<BrowserOperationResult> {
        self.state.ensure_open()?;
        let manager = ManagedBrowserManager::new(self.state.cache_dir.clone());
        let discovery = self.state.discovery.discover().await;
        let target = discovery
            .candidates
            .iter()
            .find(|candidate| {
                candidate.browser.source == BrowserSource::Managed
                    && candidate.browser.revision.as_deref() == Some(revision)
            })
            .ok_or_else(|| {
                PageKnotError::new(
                    "pageknot.browser.install",
                    ErrorStage::Browser,
                    format!("managed browser revision `{revision}` is not installed"),
                )
            })?;
        let selected = discovery
            .selected
            .as_ref()
            .is_some_and(|browser| browser == &target.browser);
        let (active_browser, active_leases) = self.state.active_browser_snapshot().await;
        let owns_target = active_browser.as_ref().is_some_and(|browser| {
            browser.source == BrowserSource::Managed
                && browser.revision.as_deref() == Some(revision)
        });
        if owns_target && active_leases > 0 {
            return Err(PageKnotError::new(
                "pageknot.browser.active",
                ErrorStage::Browser,
                format!(
                    "managed browser revision `{revision}` has {active_leases} active browser context leases"
                ),
            )
            .with_detail("revision", revision)
            .with_detail("activeLeases", active_leases));
        }
        if selected && !force {
            return Err(PageKnotError::new(
                "pageknot.browser.active",
                ErrorStage::Browser,
                format!(
                    "managed browser revision `{revision}` is selected and requires `--force` for removal"
                ),
            )
            .with_detail("revision", revision));
        }
        if selected
            && !discovery
                .candidates
                .iter()
                .any(|candidate| candidate.browser != target.browser)
        {
            return Err(PageKnotError::new(
                "pageknot.browser.unavailable",
                ErrorStage::Browser,
                "the selected managed browser cannot be removed until another compatible browser is available",
            )
            .with_detail("revision", revision)
            .with_detail("recoveryCommand", "install a compatible system browser"));
        }
        if owns_target {
            self.state.close_idle_browser().await?;
        }
        manager.remove(revision).await?;
        Ok(BrowserOperationResult {
            schema_version: pageknot_model::PUBLIC_SCHEMA_VERSION,
            action: BrowserAction::Remove,
            browser: None,
            revision: Some(revision.to_owned()),
            cache_dir: self.state.cache_dir.clone().into(),
            candidates: Vec::new(),
        })
    }

    /// Returns [`BrowserService::list`] as the stable browser-operation record
    /// used by machine-facing clients.
    pub async fn list_operation(&self) -> Result<BrowserOperationResult> {
        let candidates = self.list().await?;
        let browser = candidates
            .iter()
            .find(|candidate| candidate.state == pageknot_model::BrowserCandidateState::Selected)
            .map(|candidate| candidate.browser.clone());
        let revision = browser
            .as_ref()
            .and_then(|browser| browser.revision.clone());
        Ok(BrowserOperationResult {
            schema_version: pageknot_model::PUBLIC_SCHEMA_VERSION,
            action: BrowserAction::List,
            browser,
            revision,
            cache_dir: self.state.cache_dir.clone().into(),
            candidates,
        })
    }

    /// Reports browser, cache, collector, output, configuration, network, and
    /// recovery diagnostics.
    pub async fn doctor(&self) -> BrowserDoctorReport {
        let closed = self.state.ensure_open().is_err();
        if !closed && let Some(backend) = self.state.custom_backend() {
            return backend.doctor(self.state.default_browser()).await;
        }
        let mut discovery = self.state.discovery.discover().await;
        let remote_failure = match self.state.default_browser() {
            BrowserSpec::Remote(endpoint) if !closed => {
                for (index, candidate) in discovery.candidates.iter_mut().enumerate() {
                    candidate.state = BrowserCandidateState::Shadowed;
                    candidate.reason_code = "pageknot.browser.shadowed_by_remote".to_owned();
                    candidate.priority = u32::try_from(index.saturating_add(1)).unwrap_or(u32::MAX);
                }
                match probe_remote_browser(endpoint).await {
                    Ok(browser) => {
                        discovery.candidates.insert(
                            0,
                            BrowserCandidate {
                                browser: browser.clone(),
                                state: BrowserCandidateState::Selected,
                                reason_code: "pageknot.browser.remote".to_owned(),
                                priority: 0,
                                active_leases: 0,
                            },
                        );
                        discovery.selected = Some(browser);
                        None
                    }
                    Err(error) => {
                        discovery.selected = None;
                        Some(error)
                    }
                }
            }
            BrowserSpec::Remote(_) | BrowserSpec::Auto | BrowserSpec::Executable(_) => None,
        };
        apply_active_leases(
            &mut discovery.candidates,
            self.state.active_browser_snapshot().await,
        );
        let output = output_capability();
        let mut recovery = if closed {
            vec![RecoveryAction {
                code: "pageknot.runtime.closed".to_owned(),
                description: "Construct a new PageKnot handle.".to_owned(),
                command: String::new(),
                arguments: Vec::new(),
            }]
        } else if let Some(error) = &remote_failure {
            vec![RecoveryAction {
                code: error.code.to_string(),
                description: error.message.clone(),
                command: String::new(),
                arguments: Vec::new(),
            }]
        } else if discovery.selected.is_none() {
            vec![RecoveryAction {
                code: "pageknot.browser.install".to_owned(),
                description: "Install the managed Chromium build.".to_owned(),
                command: "pageknot".to_owned(),
                arguments: vec!["browser".to_owned(), "install".to_owned()],
            }]
        } else {
            Vec::new()
        };
        if remote_failure.is_none()
            && !discovery.failures.is_empty()
            && discovery.selected.is_none()
        {
            recovery.push(RecoveryAction {
                code: discovery.failures[0].code.to_string(),
                description: discovery.failures[0].message.clone(),
                command: "pageknot".to_owned(),
                arguments: vec!["browser".to_owned(), "install".to_owned()],
            });
        }
        let browser_constraints = discovery
            .selected
            .as_ref()
            .map_or_else(Vec::new, selected_browser_constraints);
        let browser_ready = browser_constraints.is_empty();
        recovery.extend(browser_constraints);
        let collector = if !closed {
            match discovery.selected.as_ref() {
                Some(browser) => match self.probe_selected_browser(browser).await {
                    Ok(handshake) => {
                        let (check, failure) = capability_check(&handshake);
                        if let Some(error) = failure {
                            recovery.push(probe_recovery(browser, &error));
                        }
                        check
                    }
                    Err(error) => {
                        recovery.push(probe_recovery(browser, &error));
                        untested_capability_check()
                    }
                },
                None => untested_capability_check(),
            }
        } else {
            untested_capability_check()
        };
        let ready = !closed
            && discovery.selected.is_some()
            && browser_ready
            && collector.compatible
            && output.writable;
        let manager = ManagedBrowserManager::new(self.state.cache_dir.clone());
        let mut managed_cache = match manager.state().await {
            Ok(state) => state,
            Err(error) => {
                recovery.push(RecoveryAction {
                    code: error.code.to_string(),
                    description: error.message,
                    command: "pageknot".to_owned(),
                    arguments: vec!["browser".to_owned(), "list".to_owned()],
                });
                ManagedBrowserState {
                    cache_dir: self.state.cache_dir.clone().into(),
                    installed_revisions: Vec::new(),
                    selected_revision: None,
                    catalog_version: MANAGED_BROWSER_CATALOG_VERSION.to_owned(),
                }
            }
        };
        managed_cache.selected_revision = discovery
            .selected
            .as_ref()
            .filter(|browser| browser.source == BrowserSource::Managed)
            .and_then(|browser| browser.revision.clone());
        BrowserDoctorReport {
            schema_version: pageknot_model::PUBLIC_SCHEMA_VERSION,
            ready,
            selected: discovery.selected,
            candidates: discovery.candidates,
            managed_cache,
            collector,
            output,
            configuration: effective_configuration(&self.state),
            network: network_summary(&self.state.default_network_policy),
            recovery,
        }
    }

    /// Closes an idle owned browser while keeping the service available.
    pub async fn close_idle(&self) -> Result<()> {
        self.state.close_idle_browser().await
    }

    async fn probe_selected_browser(&self, browser: &BrowserInfo) -> Result<CollectorHandshake> {
        match browser.source {
            BrowserSource::Remote => {
                let BrowserSpec::Remote(endpoint) = self.state.default_browser() else {
                    return Err(PageKnotError::new(
                        "pageknot.browser.unavailable",
                        ErrorStage::Browser,
                        "selected remote browser has no configured endpoint",
                    ));
                };
                let endpoint = resolve_remote_endpoint(endpoint).await?;
                let client = CdpClient::connect(endpoint).await?;
                let probe = probe_cdp_client(client.clone()).await;
                let close = client.close().await;
                operation_with_cleanup(probe, close)
            }
            BrowserSource::Managed | BrowserSource::System | BrowserSource::Explicit => {
                let executable = browser.executable_path.clone().ok_or_else(|| {
                    PageKnotError::new(
                        "pageknot.browser.executable",
                        ErrorStage::Browser,
                        "selected local browser has no executable path",
                    )
                })?;
                let managed_lease = if browser.source == BrowserSource::Managed {
                    let revision = browser.revision.as_deref().ok_or_else(|| {
                        PageKnotError::new(
                            "pageknot.browser.install",
                            ErrorStage::Browser,
                            "managed browser record omitted its revision",
                        )
                    })?;
                    Some(
                        ManagedBrowserManager::new(self.state.cache_dir.clone())
                            .lease(revision)
                            .await?,
                    )
                } else {
                    None
                };
                let process =
                    ChromiumProcess::launch(ChromiumLaunchOptions::new(executable)).await?;
                let probe = probe_cdp_client(process.client().clone()).await;
                let close = process.close().await;
                drop(managed_lease);
                operation_with_cleanup(probe, close)
            }
        }
    }
}

async fn probe_cdp_client(client: CdpClient) -> Result<CollectorHandshake> {
    tokio::time::timeout(COLLECTOR_PROBE_TIMEOUT, async move {
        let page = ChromiumPage::create(client, &BrowserEnvironment::default()).await?;
        let capture_id = CaptureId::new();
        let probe = probe_collector_handshake(
            &page,
            page.session_id(),
            &capture_id,
            COLLECTOR_PROBE_CHUNK_BYTES,
        )
        .await;
        let close = page.close().await;
        operation_with_cleanup(probe, close)
    })
    .await
    .map_err(|_| {
        PageKnotError::new(
            "pageknot.browser.cdp_timeout",
            ErrorStage::Browser,
            "browser readiness probe exceeded its deadline",
        )
        .retryable(true)
    })?
}

fn operation_with_cleanup<T>(operation: Result<T>, cleanup: Result<()>) -> Result<T> {
    match (operation, cleanup) {
        (Ok(value), Ok(())) => Ok(value),
        (Err(error), _) | (Ok(_), Err(error)) => Err(error),
    }
}

fn untested_capability_check() -> CapabilityCheck {
    CapabilityCheck {
        compatible: false,
        host_version: COLLECTOR_PROTOCOL_VERSION_STRING.to_owned(),
        peer_version: None,
        capabilities: Vec::new(),
        missing_capabilities: Vec::new(),
    }
}

fn capability_check(handshake: &CollectorHandshake) -> (CapabilityCheck, Option<PageKnotError>) {
    let capabilities = handshake
        .available_capabilities
        .iter()
        .map(|capability| capability.as_str().to_owned())
        .collect();
    let missing_capabilities = handshake
        .requested_capabilities
        .difference(&handshake.available_capabilities)
        .map(|capability| capability.as_str().to_owned())
        .collect();
    let negotiation = negotiate_protocol(
        handshake,
        COLLECTOR_PROTOCOL_VERSION,
        COLLECTOR_PROBE_CHUNK_BYTES,
    );
    let compatible = negotiation.is_ok();
    let failure = negotiation.err();
    (
        CapabilityCheck {
            compatible,
            host_version: COLLECTOR_PROTOCOL_VERSION_STRING.to_owned(),
            peer_version: Some(format!(
                "{}.{}",
                handshake.protocol.major, handshake.protocol.minor
            )),
            capabilities,
            missing_capabilities,
        },
        failure,
    )
}

fn probe_recovery(browser: &BrowserInfo, error: &PageKnotError) -> RecoveryAction {
    if browser.source != BrowserSource::Remote
        && error.code.as_str().starts_with("pageknot.browser.")
    {
        return RecoveryAction {
            code: error.code.to_string(),
            description: error.message.clone(),
            command: "pageknot".to_owned(),
            arguments: vec!["browser".to_owned(), "install".to_owned()],
        };
    }
    let recovery = if browser.source == BrowserSource::Remote {
        " Check the remote CDP permissions for isolated contexts and Runtime evaluation."
    } else {
        " Reinstall PageKnot from one release and run doctor again."
    };
    RecoveryAction {
        code: error.code.to_string(),
        description: format!("{}{recovery}", error.message),
        command: String::new(),
        arguments: Vec::new(),
    }
}

fn apply_active_leases(
    candidates: &mut [BrowserCandidate],
    (active_browser, active_leases): (Option<BrowserInfo>, u32),
) {
    let Some(active_browser) = active_browser else {
        return;
    };
    for candidate in candidates {
        if candidate.browser == active_browser {
            candidate.active_leases = candidate.active_leases.max(active_leases);
        }
    }
}

fn selected_browser_constraints(browser: &BrowserInfo) -> Vec<RecoveryAction> {
    if browser.source != BrowserSource::Remote {
        return Vec::new();
    }
    vec![
        RecoveryAction {
            code: "pageknot.input.remote_network_policy".to_owned(),
            description: "Remote CDP supports only the unrestricted network policy because the endpoint does not declare trusted proxy enforcement. Use a PageKnot-owned browser when capture requires address policy enforcement.".to_owned(),
            command: String::new(),
            arguments: Vec::new(),
        },
        RecoveryAction {
            code: "pageknot.browser.offline_verifier_unavailable".to_owned(),
            description: "Browser-backed offline verification requires a PageKnot-owned browser because the remote endpoint does not declare trusted proxy enforcement.".to_owned(),
            command: String::new(),
            arguments: Vec::new(),
        },
    ]
}

fn effective_configuration(state: &RuntimeState) -> Vec<EffectiveConfigValue> {
    let mut configuration = state.effective_configuration.clone();
    if !configuration
        .iter()
        .any(|value| value.field == "browser.cacheDir")
    {
        configuration.push(EffectiveConfigValue {
            field: "browser.cacheDir".to_owned(),
            value: serde_json::Value::String(state.cache_dir.to_string()),
            provenance: ConfigProvenance::Default,
            redacted: false,
        });
    }
    if !configuration
        .iter()
        .any(|value| value.field == "browser.channel")
    {
        configuration.push(EffectiveConfigValue {
            field: "browser.channel".to_owned(),
            value: serde_json::to_value(state.browser_channel).unwrap_or(serde_json::Value::Null),
            provenance: ConfigProvenance::Default,
            redacted: false,
        });
    }
    if !configuration
        .iter()
        .any(|value| value.field == "browser.installation")
    {
        configuration.push(EffectiveConfigValue {
            field: "browser.installation".to_owned(),
            value: serde_json::to_value(state.browser_installation)
                .unwrap_or(serde_json::Value::Null),
            provenance: ConfigProvenance::Default,
            redacted: false,
        });
    }
    configuration.sort_by(|left, right| left.field.cmp(&right.field));
    configuration
}

fn network_summary(policy: &pageknot_model::NetworkPolicy) -> NetworkPolicySummary {
    match policy {
        pageknot_model::NetworkPolicy::Standard => NetworkPolicySummary {
            profile: "standard".to_owned(),
            permits_loopback_initial_origin: true,
            permits_private_addresses: false,
            revalidates_redirects: true,
        },
        pageknot_model::NetworkPolicy::Server => NetworkPolicySummary {
            profile: "server".to_owned(),
            permits_loopback_initial_origin: false,
            permits_private_addresses: false,
            revalidates_redirects: true,
        },
        pageknot_model::NetworkPolicy::Unrestricted => NetworkPolicySummary {
            profile: "unrestricted".to_owned(),
            permits_loopback_initial_origin: true,
            permits_private_addresses: true,
            revalidates_redirects: true,
        },
        pageknot_model::NetworkPolicy::Custom(rules) => NetworkPolicySummary {
            profile: "custom".to_owned(),
            permits_loopback_initial_origin: rules.allow_loopback,
            permits_private_addresses: rules.allow_private,
            revalidates_redirects: true,
        },
    }
}

fn output_capability() -> OutputCapability {
    let directory = std::env::current_dir()
        .ok()
        .and_then(|path| Utf8PathBuf::from_path_buf(path).ok())
        .unwrap_or_else(|| Utf8PathBuf::from("."));
    let result = tempfile::Builder::new()
        .prefix(".pageknot-doctor-")
        .tempfile_in(&directory);
    match result {
        Ok(file) => {
            drop(file);
            OutputCapability {
                directory: directory.into(),
                writable: true,
                atomic_create: true,
                atomic_replace: true,
                reason_code: None,
            }
        }
        Err(error) => OutputCapability {
            directory: directory.into(),
            writable: false,
            atomic_create: false,
            atomic_replace: false,
            reason_code: Some(format!("pageknot.output.{}", error.kind() as u8)),
        },
    }
}

#[allow(dead_code)]
fn _browser_spec_is_public(_: BrowserSpec) {}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use pageknot_model::{BrowserInfo, BrowserProduct, BrowserSource, CaptureId, ContentDigest};
    use pageknot_protocol::{CollectorCapability, CollectorHandshake, ProtocolVersion};

    use super::{capability_check, selected_browser_constraints, untested_capability_check};

    fn handshake(
        protocol: ProtocolVersion,
        available_capabilities: BTreeSet<CollectorCapability>,
    ) -> CollectorHandshake {
        CollectorHandshake {
            protocol,
            capture_id: CaptureId::new(),
            host_build_sha256: ContentDigest::sha256(b"host"),
            collector_build_sha256: ContentDigest::sha256(b"collector"),
            requested_capabilities: BTreeSet::from(CollectorCapability::ALL),
            available_capabilities,
            maximum_chunk_bytes: 64 * 1024,
        }
    }

    #[test]
    fn compatible_handshake_reports_the_observed_collector_capabilities() {
        let (check, failure) = capability_check(&handshake(
            ProtocolVersion { major: 1, minor: 5 },
            BTreeSet::from(CollectorCapability::ALL),
        ));

        assert!(check.compatible);
        assert_eq!(check.peer_version.as_deref(), Some("1.5"));
        assert_eq!(check.capabilities.len(), CollectorCapability::ALL.len());
        assert!(check.missing_capabilities.is_empty());
        assert!(failure.is_none());
    }

    #[test]
    fn incompatible_handshake_reports_the_peer_version() {
        let (check, failure) = capability_check(&handshake(
            ProtocolVersion { major: 2, minor: 0 },
            BTreeSet::from(CollectorCapability::ALL),
        ));

        assert!(!check.compatible);
        assert_eq!(check.peer_version.as_deref(), Some("2.0"));
        assert_eq!(
            failure.as_ref().map(|error| error.code.as_str()),
            Some("pageknot.collector.protocol_major")
        );
    }

    #[test]
    fn capability_gap_is_named_in_the_doctor_record() {
        let available = CollectorCapability::ALL
            .into_iter()
            .filter(|capability| *capability != CollectorCapability::CanvasPixels)
            .collect();
        let (check, failure) = capability_check(&handshake(
            ProtocolVersion { major: 1, minor: 5 },
            available,
        ));

        assert!(!check.compatible);
        assert_eq!(check.missing_capabilities, vec!["canvas-pixels".to_owned()]);
        assert_eq!(
            failure.as_ref().map(|error| error.code.as_str()),
            Some("pageknot.collector.capability")
        );
    }

    #[test]
    fn unavailable_probe_is_reported_as_untested() {
        let check = untested_capability_check();

        assert!(!check.compatible);
        assert!(check.peer_version.is_none());
        assert!(check.capabilities.is_empty());
        assert!(check.missing_capabilities.is_empty());
    }

    #[test]
    fn remote_browser_constraints_name_network_and_offline_guarantees() {
        let constraints = selected_browser_constraints(&BrowserInfo {
            product: BrowserProduct::Chrome,
            version: "151.0.0.0".to_owned(),
            source: BrowserSource::Remote,
            executable_path: None,
            endpoint: None,
            revision: None,
            protocol_version: "1.3".to_owned(),
        });

        assert_eq!(
            constraints
                .iter()
                .map(|constraint| constraint.code.as_str())
                .collect::<Vec<_>>(),
            [
                "pageknot.input.remote_network_policy",
                "pageknot.browser.offline_verifier_unavailable"
            ]
        );
        assert!(
            constraints[0]
                .description
                .contains("supports only the unrestricted network policy")
        );
        assert!(
            constraints[1]
                .description
                .contains("trusted proxy enforcement")
        );
        assert!(
            constraints
                .iter()
                .all(|constraint| constraint.command.is_empty())
        );
    }
}
