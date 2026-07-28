use std::time::Duration;

use pageknot_model::{
    ArtifactSpec, ArtifactTarget, CaptureRequest, CaptureResult, CaptureScope, ConflictPolicy,
    Milliseconds, MissingResourcePolicy, OptimizationPolicy, PortablePath, ReadinessMode, Result,
    Viewport,
};

use crate::{CaptureJob, CaptureService};

#[derive(Clone, Debug)]
/// A one-shot capture request with ergonomic policy setters.
pub struct CaptureBuilder {
    service: CaptureService,
    request: CaptureRequest,
}

impl CaptureBuilder {
    pub(crate) const fn new(service: CaptureService, request: CaptureRequest) -> Self {
        Self { service, request }
    }

    /// Writes the verified artifact to `path`, replacing an existing file after
    /// verification succeeds.
    #[must_use]
    pub fn output(mut self, path: impl Into<PortablePath>) -> Self {
        let ArtifactSpec::Html(spec) = &mut self.request.artifact;
        spec.target = ArtifactTarget::File(path.into());
        self
    }

    /// Applies a profile registered on the parent [`crate::PageKnot`] service.
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
        self.request.capture.missing_resources = MissingResourcePolicy::Fail;
        self
    }

    /// Selects the complete page or its active top-level selection.
    ///
    /// This replaces a selector set earlier on the builder.
    #[must_use]
    pub fn scope(mut self, scope: CaptureScope) -> Self {
        self.request.capture.scope = scope;
        self.request.capture.selector = None;
        self
    }

    /// Captures the first top-level document element matching `selector`.
    ///
    /// The browser reports a collection error when the selector is invalid or
    /// no element matches.
    #[must_use]
    pub fn selector(mut self, selector: impl Into<String>) -> Self {
        self.request.capture.scope = CaptureScope::Page;
        self.request.capture.selector = Some(selector.into());
        self
    }

    /// Applies browser-observed document optimizers.
    #[must_use]
    pub const fn optimizations(mut self, policy: OptimizationPolicy) -> Self {
        self.request.capture.optimizations = policy;
        self
    }

    /// Removes CSS rules that cannot match the captured document state.
    #[must_use]
    pub const fn remove_unused_css(mut self) -> Self {
        self.request.capture.optimizations.remove_unused_css = true;
        self
    }

    /// Removes font faces unused by the captured document state.
    #[must_use]
    pub const fn remove_unused_fonts(mut self) -> Self {
        self.request.capture.optimizations.remove_unused_fonts = true;
        self
    }

    /// Removes elements whose computed display is none.
    #[must_use]
    pub const fn remove_hidden_elements(mut self) -> Self {
        self.request.capture.optimizations.remove_hidden_elements = true;
        self
    }

    /// Selects output conflict behavior for file artifacts.
    #[must_use]
    pub fn conflict(mut self, conflict: ConflictPolicy) -> Self {
        let ArtifactSpec::Html(spec) = &mut self.request.artifact;
        spec.conflict = conflict;
        self
    }

    /// Writes, verifies, and commits an HTML artifact to `path`, replacing an
    /// existing file after verification succeeds.
    pub async fn save(mut self, path: impl Into<PortablePath>) -> Result<CaptureResult> {
        let ArtifactSpec::Html(spec) = &mut self.request.artifact;
        spec.target = ArtifactTarget::File(path.into());
        self.run().await
    }

    #[allow(clippy::wrong_self_convention)]
    /// Returns a verified HTML artifact in memory.
    ///
    /// Encoding fails before returning bytes when the artifact exceeds
    /// `maximum_bytes`.
    pub async fn to_bytes(mut self, maximum_bytes: u64) -> Result<CaptureResult> {
        let ArtifactSpec::Html(spec) = &mut self.request.artifact;
        spec.target = ArtifactTarget::Bytes {
            max_bytes: maximum_bytes,
        };
        self.run().await
    }

    /// Starts the capture and returns its job handle.
    pub async fn start(self) -> Result<CaptureJob> {
        self.service.start(self.request).await
    }

    /// Starts the capture and waits for its terminal result.
    pub async fn run(self) -> Result<CaptureResult> {
        self.start().await?.wait().await
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use pageknot_model::{
        ArtifactSpec, CaptureScope, ConflictPolicy, MissingResourcePolicy, NetworkPolicy,
        ReadinessMode,
    };

    use crate::PageKnot;

    #[test]
    fn one_shot_builder_applies_named_profiles_before_start() {
        let pageknot = PageKnot::builder().build();
        let builder = pageknot
            .as_ref()
            .ok()
            .map(|pageknot| pageknot.capture("https://example.com"))
            .and_then(std::result::Result::ok)
            .map(|builder| builder.profile("server"));
        let builder = builder.and_then(std::result::Result::ok);

        assert_eq!(
            builder
                .as_ref()
                .map(|builder| builder.request.capture.missing_resources),
            Some(MissingResourcePolicy::Fail)
        );
        assert!(matches!(
            builder.as_ref().map(|builder| &builder.request.network),
            Some(NetworkPolicy::Server)
        ));
    }

    #[test]
    fn selector_replaces_active_selection_scope() {
        let builder = PageKnot::builder()
            .build()
            .and_then(|pageknot| pageknot.capture("https://example.com"))
            .map(|builder| builder.scope(CaptureScope::Selection).selector("article"));

        assert_eq!(
            builder
                .as_ref()
                .map(|builder| builder.request.capture.scope),
            Ok(CaptureScope::Page)
        );
        assert_eq!(
            builder
                .as_ref()
                .ok()
                .and_then(|builder| builder.request.capture.selector.as_deref()),
            Some("article")
        );
    }

    #[test]
    fn one_shot_file_output_defaults_to_atomic_replacement() {
        let builder = PageKnot::builder()
            .build()
            .and_then(|pageknot| pageknot.capture("https://example.com"))
            .map(|builder| builder.output("capture.html"));
        let conflict = builder.as_ref().map(|builder| {
            let ArtifactSpec::Html(artifact) = &builder.request.artifact;
            artifact.conflict
        });

        assert_eq!(conflict, Ok(ConflictPolicy::Replace));
    }

    #[test]
    fn one_shot_builder_configures_readiness_and_delay() {
        let builder = PageKnot::builder()
            .build()
            .and_then(|pageknot| pageknot.capture("https://example.com"))
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
    fn pageknot_builder_rejects_an_undefined_default_profile() {
        let result = PageKnot::builder().profile("undefined").build();

        assert_eq!(
            result.err().map(|error| error.code),
            Some(pageknot_model::ErrorCode::from_static(
                "pageknot.config.profile"
            ))
        );
    }

    #[test]
    fn pageknot_builder_rejects_a_zero_browser_recycling_threshold() {
        let result = PageKnot::builder().browser_recycle_after_jobs(0).build();

        assert_eq!(
            result.err().map(|error| error.code),
            Some(pageknot_model::ErrorCode::from_static(
                "pageknot.input.browser_recycle"
            ))
        );
    }
}
