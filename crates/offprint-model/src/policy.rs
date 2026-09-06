use std::collections::BTreeSet;
use std::time::Duration;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub const MAXIMUM_CAPTURE_NODES: u64 = 1_000_000;

#[derive(
    Clone, Copy, Debug, Default, Deserialize, Eq, JsonSchema, Ord, PartialEq, PartialOrd, Serialize,
)]
#[serde(transparent)]
pub struct Milliseconds(pub u64);

impl Milliseconds {
    #[must_use]
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

impl From<Duration> for Milliseconds {
    fn from(value: Duration) -> Self {
        Self(u64::try_from(value.as_millis()).unwrap_or(u64::MAX))
    }
}

impl From<Milliseconds> for Duration {
    fn from(value: Milliseconds) -> Self {
        Self::from_millis(value.0)
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ColorScheme {
    Light,
    Dark,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ReducedMotion {
    Reduce,
    NoPreference,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(tag = "kind", content = "value", rename_all = "kebab-case")]
pub enum UserAgentPolicy {
    BrowserDefault,
    Override(String),
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Viewport {
    pub width: u32,
    pub height: u32,
    pub scale: u8,
}

impl Default for Viewport {
    fn default() -> Self {
        Self {
            width: 1440,
            height: 900,
            scale: 1,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BrowserEnvironment {
    pub viewport: Viewport,
    pub locale: String,
    pub timezone: String,
    pub color_scheme: ColorScheme,
    pub reduced_motion: ReducedMotion,
    pub user_agent: UserAgentPolicy,
}

impl Default for BrowserEnvironment {
    fn default() -> Self {
        Self {
            viewport: Viewport::default(),
            locale: "en-US".to_owned(),
            timezone: "UTC".to_owned(),
            color_scheme: ColorScheme::Light,
            reduced_motion: ReducedMotion::Reduce,
            user_agent: UserAgentPolicy::BrowserDefault,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum MissingResourcePolicy {
    Warn,
    Fail,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum CaptureScope {
    #[default]
    Page,
    Selection,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(default, rename_all = "camelCase")]
pub struct OptimizationPolicy {
    pub remove_unused_css: bool,
    pub remove_unused_fonts: bool,
    pub remove_hidden_elements: bool,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum VerificationMode {
    Static,
    Offline,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NetworkRules {
    #[serde(default)]
    pub allowed_hosts: BTreeSet<String>,
    #[serde(default)]
    pub allowed_cidrs: BTreeSet<String>,
    #[serde(default)]
    pub allow_loopback: bool,
    #[serde(default)]
    pub allow_private: bool,
    #[serde(default)]
    pub allow_link_local: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(tag = "kind", content = "rules", rename_all = "kebab-case")]
pub enum NetworkPolicy {
    Standard,
    Server,
    Unrestricted,
    Custom(NetworkRules),
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ConflictPolicy {
    Fail,
    Replace,
    Uniquify,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ViewportSweepPolicy {
    pub step_pixels: u32,
    pub settle: Milliseconds,
    pub max_steps: u32,
}

impl Default for ViewportSweepPolicy {
    fn default() -> Self {
        Self {
            step_pixels: 720,
            settle: Milliseconds(100),
            max_steps: 100,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(tag = "kind", content = "options", rename_all = "kebab-case")]
pub enum LazyLoadPolicy {
    Disabled,
    ViewportSweep(ViewportSweepPolicy),
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ReadinessMode {
    RenderIdle,
    NetworkIdle,
    Load,
    DomContentLoaded,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReadinessPolicy {
    pub mode: ReadinessMode,
    pub network_quiet: Milliseconds,
    pub mutation_quiet: Milliseconds,
    pub delay: Milliseconds,
    pub lazy_load: LazyLoadPolicy,
}

impl Default for ReadinessPolicy {
    fn default() -> Self {
        Self {
            mode: ReadinessMode::RenderIdle,
            network_quiet: Milliseconds(500),
            mutation_quiet: Milliseconds(300),
            delay: Milliseconds(0),
            lazy_load: LazyLoadPolicy::ViewportSweep(ViewportSweepPolicy::default()),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ContentPolicy {
    pub missing_resources: MissingResourcePolicy,
    pub preserve_password_values: bool,
    #[serde(default)]
    pub scope: CaptureScope,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selector: Option<String>,
    #[serde(default)]
    pub optimizations: OptimizationPolicy,
    #[serde(default)]
    pub allowed_file_roots: Vec<crate::PortablePath>,
}

impl Default for ContentPolicy {
    fn default() -> Self {
        Self {
            missing_resources: MissingResourcePolicy::Warn,
            preserve_password_values: false,
            scope: CaptureScope::Page,
            selector: None,
            optimizations: OptimizationPolicy::default(),
            allowed_file_roots: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CaptureLimits {
    pub duration: Milliseconds,
    pub redirects: u32,
    pub frames: u32,
    #[schemars(range(min = 1, max = MAXIMUM_CAPTURE_NODES))]
    pub nodes: u64,
    pub resources: u32,
    pub resource_bytes: u64,
    pub total_resource_bytes: u64,
    pub collector_chunk_bytes: u64,
    pub concurrent_resources: u16,
    pub artifact_bytes: u64,
    pub css_import_depth: u16,
    pub frame_depth: u16,
}

impl Default for CaptureLimits {
    fn default() -> Self {
        Self {
            duration: Milliseconds(120_000),
            redirects: 20,
            frames: 256,
            nodes: MAXIMUM_CAPTURE_NODES,
            resources: 10_000,
            resource_bytes: 64 * 1024 * 1024,
            total_resource_bytes: 512 * 1024 * 1024,
            collector_chunk_bytes: 1024 * 1024,
            concurrent_resources: 8,
            artifact_bytes: 64 * 1024 * 1024,
            css_import_depth: 64,
            frame_depth: 64,
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DiagnosticsPolicy {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub directory: Option<crate::PortablePath>,
    #[serde(default)]
    pub screenshots: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CaptureProfile {
    pub environment: BrowserEnvironment,
    pub readiness: ReadinessPolicy,
    pub content: ContentPolicy,
    pub network: NetworkPolicy,
    pub limits: CaptureLimits,
    pub verification: VerificationMode,
}

impl CaptureProfile {
    pub fn named(name: &str) -> crate::Result<Self> {
        let mut profile = Self::default();
        match name {
            "default" => {}
            "strict" => {
                profile.content.missing_resources = MissingResourcePolicy::Fail;
            }
            "server" => {
                profile.content.missing_resources = MissingResourcePolicy::Fail;
                profile.network = NetworkPolicy::Server;
            }
            _ => {
                return Err(crate::OffprintError::new(
                    "offprint.config.profile",
                    crate::ErrorStage::Validation,
                    format!("capture profile `{name}` is undefined"),
                ));
            }
        }
        Ok(profile)
    }

    pub fn apply_to(&self, request: &mut crate::CaptureRequest) {
        let inherits_artifact_limit = matches!(
            request.output,
            crate::CaptureOutput::Memory { max_bytes }
                if max_bytes == request.limits.artifact_bytes
        );
        request.environment = self.environment.clone();
        request.readiness = self.readiness.clone();
        request.content = self.content.clone();
        request.network = self.network.clone();
        request.limits = self.limits.clone();
        request.verification = self.verification;
        if inherits_artifact_limit {
            request.output = crate::CaptureOutput::Memory {
                max_bytes: self.limits.artifact_bytes,
            };
        }
    }
}

impl Default for CaptureProfile {
    fn default() -> Self {
        Self {
            environment: BrowserEnvironment::default(),
            readiness: ReadinessPolicy::default(),
            content: ContentPolicy::default(),
            network: NetworkPolicy::Standard,
            limits: CaptureLimits::default(),
            verification: VerificationMode::Offline,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{CaptureOutput, CaptureProfile, CaptureRequest};

    #[test]
    fn profile_updates_an_inherited_memory_artifact_limit() -> crate::Result<()> {
        let mut request =
            CaptureRequest::builder("https://example.com").and_then(|builder| builder.build())?;
        let mut profile = CaptureProfile::default();
        profile.limits.artifact_bytes = 4096;

        profile.apply_to(&mut request);

        assert_eq!(request.output, CaptureOutput::Memory { max_bytes: 4096 });
        Ok(())
    }

    #[test]
    fn profile_preserves_an_explicit_memory_artifact_limit() -> crate::Result<()> {
        let mut request =
            CaptureRequest::builder("https://example.com").and_then(|builder| builder.build())?;
        request.output = CaptureOutput::memory(2048);
        let mut profile = CaptureProfile::default();
        profile.limits.artifact_bytes = 4096;

        profile.apply_to(&mut request);

        assert_eq!(request.output, CaptureOutput::Memory { max_bytes: 2048 });
        Ok(())
    }
}
