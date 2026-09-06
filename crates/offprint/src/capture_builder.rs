use std::time::Duration;

use offprint_model::{
    CaptureOutput, CaptureReceipt, CaptureRequest, CaptureScope, ConflictPolicy, Milliseconds,
    MissingResourcePolicy, NetworkPolicy, OptimizationPolicy, PortablePath, ReadinessMode, Result,
    VerificationMode, Viewport,
};

use crate::{CaptureJob, CaptureService};

#[derive(Clone, Debug)]
/// A one-shot capture request with ergonomic policy setters.
pub struct Capture {
    service: CaptureService,
    request: CaptureRequest,
    conflict: ConflictPolicy,
}

impl Capture {
    pub(crate) const fn new(service: CaptureService, request: CaptureRequest) -> Self {
        Self {
            service,
            request,
            conflict: ConflictPolicy::Fail,
        }
    }

    /// Applies a profile registered on the parent [`crate::Offprint`] service.
    pub fn profile(mut self, name: impl AsRef<str>) -> Result<Self> {
        let profile = self.service.profile(name.as_ref())?;
        profile.apply_to(&mut self.request);
        Ok(self)
    }

    /// Sets the total capture deadline.
    #[must_use]
    pub fn timeout(mut self, duration: Duration) -> Self {
        self.request.limits.duration = Milliseconds::from(duration);
        self
    }

    /// Selects the browser condition that must complete before capture.
    #[must_use]
    pub const fn wait_until(mut self, mode: ReadinessMode) -> Self {
        self.request.readiness.mode = mode;
        self
    }

    /// Waits for `duration` after the selected readiness condition.
    #[must_use]
    pub fn delay(mut self, duration: Duration) -> Self {
        self.request.readiness.delay = Milliseconds::from(duration);
        self
    }

    /// Sets the emulated viewport.
    #[must_use]
    pub fn viewport(mut self, viewport: Viewport) -> Self {
        self.request.environment.viewport = viewport;
        self
    }

    /// Overrides the service-level headed browser setting for this capture.
    #[must_use]
    pub const fn headed(mut self, headed: bool) -> Self {
        self.request.headed = Some(headed);
        self
    }

    /// Fails the capture when any discovered resource cannot be embedded.
    #[must_use]
    pub fn strict(mut self) -> Self {
        self.request.content.missing_resources = MissingResourcePolicy::Fail;
        self
    }

    /// Selects address and redirect restrictions for browser requests.
    #[must_use]
    pub fn network(mut self, policy: NetworkPolicy) -> Self {
        self.request.network = policy;
        self
    }

    /// Selects static checks or a network-denied browser reopen.
    #[must_use]
    pub const fn verification(mut self, mode: VerificationMode) -> Self {
        self.request.verification = mode;
        self
    }

    /// Selects the complete page or its active top-level selection.
    ///
    /// This replaces a selector set earlier on the builder.
    #[must_use]
    pub fn scope(mut self, scope: CaptureScope) -> Self {
        self.request.content.scope = scope;
        self.request.content.selector = None;
        self
    }

    /// Captures the first top-level document element matching `selector`.
    ///
    /// The browser reports a collection error when the selector is invalid or
    /// no element matches.
    #[must_use]
    pub fn selector(mut self, selector: impl Into<String>) -> Self {
        self.request.content.scope = CaptureScope::Page;
        self.request.content.selector = Some(selector.into());
        self
    }

    /// Applies browser-observed document optimizers.
    #[must_use]
    pub const fn optimizations(mut self, policy: OptimizationPolicy) -> Self {
        self.request.content.optimizations = policy;
        self
    }

    /// Removes CSS rules that cannot match the captured document state.
    #[must_use]
    pub const fn remove_unused_css(mut self) -> Self {
        self.request.content.optimizations.remove_unused_css = true;
        self
    }

    /// Removes font faces unused by the captured document state.
    #[must_use]
    pub const fn remove_unused_fonts(mut self) -> Self {
        self.request.content.optimizations.remove_unused_fonts = true;
        self
    }

    /// Removes elements whose computed display is none.
    #[must_use]
    pub const fn remove_hidden_elements(mut self) -> Self {
        self.request.content.optimizations.remove_hidden_elements = true;
        self
    }

    /// Selects output conflict behavior for file artifacts.
    #[must_use]
    pub fn conflict(mut self, conflict: ConflictPolicy) -> Self {
        self.conflict = conflict;
        self
    }

    /// Writes, verifies, and commits an HTML artifact to `path`.
    ///
    /// The default conflict policy returns an error when `path` exists. Call
    /// [`Self::conflict`] to select replacement or a unique destination.
    pub async fn save(mut self, path: impl Into<PortablePath>) -> Result<CaptureReceipt> {
        self.request.output = CaptureOutput::File {
            path: path.into(),
            conflict: self.conflict,
        };
        self.start().await?.result().await
    }

    #[allow(clippy::wrong_self_convention)]
    /// Returns a verified HTML artifact in memory.
    ///
    /// Encoding fails before returning bytes when the artifact exceeds
    /// `maximum_bytes`.
    pub async fn bytes(mut self, maximum_bytes: u64) -> Result<CaptureReceipt> {
        self.request.output = CaptureOutput::Memory {
            max_bytes: maximum_bytes,
        };
        self.start().await?.result().await
    }

    /// Starts the capture and returns its job handle.
    pub async fn start(self) -> Result<CaptureJob> {
        self.service.start(self.request).await
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use offprint_model::{
        CaptureScope, ConflictPolicy, MissingResourcePolicy, NetworkPolicy, ReadinessMode,
    };

    use crate::Offprint;

    #[test]
    fn one_shot_builder_applies_named_profiles_before_start() {
        let offprint = Offprint::builder().build();
        let builder = offprint
            .as_ref()
            .ok()
            .map(|offprint| offprint.capture("https://example.com"))
            .and_then(std::result::Result::ok)
            .map(|builder| builder.profile("server"));
        let builder = builder.and_then(std::result::Result::ok);

        assert_eq!(
            builder
                .as_ref()
                .map(|builder| builder.request.content.missing_resources),
            Some(MissingResourcePolicy::Fail)
        );
        assert!(matches!(
            builder.as_ref().map(|builder| &builder.request.network),
            Some(NetworkPolicy::Server)
        ));
    }

    #[test]
    fn selector_replaces_active_selection_scope() {
        let builder = Offprint::builder()
            .build()
            .and_then(|offprint| offprint.capture("https://example.com"))
            .map(|builder| builder.scope(CaptureScope::Selection).selector("article"));

        assert_eq!(
            builder
                .as_ref()
                .map(|builder| builder.request.content.scope),
            Ok(CaptureScope::Page)
        );
        assert_eq!(
            builder
                .as_ref()
                .ok()
                .and_then(|builder| builder.request.content.selector.as_deref()),
            Some("article")
        );
    }

    #[test]
    fn one_shot_file_output_defaults_to_conflict_failure() {
        let builder = Offprint::builder()
            .build()
            .and_then(|offprint| offprint.capture("https://example.com"));
        let conflict = builder.as_ref().map(|builder| builder.conflict);

        assert_eq!(conflict, Ok(ConflictPolicy::Fail));
    }

    #[test]
    fn browser_selection_conflicts_are_order_independent() {
        let endpoint =
            url::Url::parse("http://127.0.0.1:9222").unwrap_or_else(|_| std::process::abort());
        let path_then_remote = Offprint::builder()
            .browser_path("/browser")
            .cdp_url(endpoint.clone())
            .build();
        let remote_then_path = Offprint::builder()
            .cdp_url(endpoint)
            .browser_path("/browser")
            .build();

        assert_eq!(
            path_then_remote
                .err()
                .map(|error| error.code.as_str().to_owned()),
            Some("offprint.input.browser_selection".to_owned())
        );
        assert_eq!(
            remote_then_path
                .err()
                .map(|error| error.code.as_str().to_owned()),
            Some("offprint.input.browser_selection".to_owned())
        );
    }

    #[test]
    fn one_shot_builder_configures_readiness_and_delay() {
        let builder = Offprint::builder()
            .build()
            .and_then(|offprint| offprint.capture("https://example.com"))
            .map(|builder| {
                builder
                    .wait_until(ReadinessMode::NetworkIdle)
                    .delay(Duration::from_millis(750))
            });

        assert_eq!(
            builder
                .as_ref()
                .map(|builder| builder.request.readiness.mode),
            Ok(ReadinessMode::NetworkIdle)
        );
        assert_eq!(
            builder
                .as_ref()
                .map(|builder| builder.request.readiness.delay.get()),
            Ok(750)
        );
    }

    #[test]
    fn offprint_builder_rejects_an_undefined_default_profile() {
        let result = Offprint::builder().profile("undefined").build();

        assert_eq!(
            result.err().map(|error| error.code),
            Some(offprint_model::ErrorCode::from_static(
                "offprint.config.profile"
            ))
        );
    }

    #[test]
    fn offprint_builder_rejects_a_zero_browser_recycling_threshold() {
        let result = Offprint::builder().browser_recycle_after_jobs(0).build();

        assert_eq!(
            result.err().map(|error| error.code),
            Some(offprint_model::ErrorCode::from_static(
                "offprint.input.browser_recycle"
            ))
        );
    }
}
