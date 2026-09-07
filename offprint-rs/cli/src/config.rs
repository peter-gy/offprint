use std::collections::BTreeMap;

use offprint::{
    BrowserInstallationPolicy, BrowserSourcePolicy, CaptureProfile, CaptureRequest,
    ConfigProvenance, EffectiveConfigValue, LazyLoadPolicy, OffprintBuilder, OffprintError,
    ReadinessMode, ReadinessPolicy, Result,
};
use serde_json::Value;
use url::Url;

mod browser;
mod document;
mod environment;
mod profile;
mod resolve;
pub(crate) mod value;

use browser::{browser_spec, parse_cdp_url, set_browser_selection};
#[cfg(test)]
use document::ConfigFile;
#[cfg(test)]
use resolve::resolve_documents;

const MAXIMUM_CONFIG_BYTES: u64 = 1024 * 1024;

#[derive(Clone, Debug)]
pub(crate) enum BrowserSelection {
    Auto,
    Executable(String),
    Remote(Url),
}

#[derive(Clone, Debug)]
pub(crate) struct ResolvedConfig {
    browser: BrowserSelection,
    cache_dir: Option<String>,
    browser_source: BrowserSourcePolicy,
    browser_installation: BrowserInstallationPolicy,
    headless: bool,
    maximum_contexts: u16,
    profile: CaptureProfile,
    headers_path: Option<String>,
    cookies_path: Option<String>,
    configuration: BTreeMap<String, EffectiveConfigValue>,
}

impl ResolvedConfig {
    pub(crate) fn load(
        explicit_path: Option<&str>,
        selected_profile: Option<&str>,
    ) -> Result<Self> {
        resolve::load(explicit_path, selected_profile)
    }

    pub(crate) fn apply_to_request(&self, request: &mut CaptureRequest) {
        self.profile.apply_to(request);
        request.browser = browser_spec(&self.browser);
    }

    pub(crate) fn apply_to_builder(&self, mut builder: OffprintBuilder) -> OffprintBuilder {
        builder = builder
            .headed(!self.headless)
            .browser_source(self.browser_source)
            .browser_installation(self.browser_installation)
            .maximum_contexts(self.maximum_contexts)
            .effective_configuration(self.configuration.values().cloned().collect())
            .default_network_policy(self.profile.network.clone());
        if let Some(path) = &self.cache_dir {
            builder = builder.cache_dir(path);
        }
        match &self.browser {
            BrowserSelection::Auto => {}
            BrowserSelection::Executable(path) => {
                builder = builder.browser_path(path);
            }
            BrowserSelection::Remote(endpoint) => {
                builder = builder.cdp_url(endpoint.clone());
            }
        }
        builder
    }

    pub(crate) fn apply_browser_path_flag(&mut self, path: String) {
        set_browser_selection(
            self,
            BrowserSelection::Executable(path),
            ConfigProvenance::Flag,
        );
    }

    pub(crate) fn apply_cdp_url_flag(&mut self, value: &str) -> Result<()> {
        let endpoint = parse_cdp_url(value)?;
        set_browser_selection(
            self,
            BrowserSelection::Remote(endpoint),
            ConfigProvenance::Flag,
        );
        Ok(())
    }

    pub(crate) const fn uses_remote_browser(&self) -> bool {
        matches!(&self.browser, BrowserSelection::Remote(_))
    }

    pub(crate) fn headers_path(&self) -> Option<&str> {
        self.headers_path.as_deref()
    }

    pub(crate) fn cookies_path(&self) -> Option<&str> {
        self.cookies_path.as_deref()
    }
}

pub(crate) fn validate_environment() -> Result<()> {
    resolve::collect_environment(std::env::vars_os()).map(|_| ())
}

pub(crate) fn set_readiness_mode(readiness: &mut ReadinessPolicy, mode: ReadinessMode) {
    readiness.mode = mode;
    if mode != ReadinessMode::RenderIdle {
        readiness.lazy_load = LazyLoadPolicy::Disabled;
    }
}

fn set_secret_paths(
    resolved: &mut ResolvedConfig,
    headers: Option<String>,
    cookies: Option<String>,
    provenance: ConfigProvenance,
) {
    if let Some(headers) = headers {
        resolved.headers_path = Some(headers);
        record(
            resolved,
            "headers",
            Value::String("[configured]".to_owned()),
            provenance,
            true,
        );
    }
    if let Some(cookies) = cookies {
        resolved.cookies_path = Some(cookies);
        record(
            resolved,
            "cookies",
            Value::String("[configured]".to_owned()),
            provenance,
            true,
        );
    }
}

fn record(
    resolved: &mut ResolvedConfig,
    field: &str,
    value: Value,
    provenance: ConfigProvenance,
    redacted: bool,
) {
    resolved.configuration.insert(
        field.to_owned(),
        EffectiveConfigValue {
            field: field.to_owned(),
            value,
            provenance,
            redacted,
        },
    );
}

fn config_value_error(name: &str, value: &str) -> OffprintError {
    config_error(
        "offprint.config.value",
        format!("configuration {name} value `{value}` is invalid"),
    )
}

fn config_error(code: &'static str, message: impl Into<String>) -> OffprintError {
    OffprintError::new(code, offprint::ErrorStage::Validation, message)
}

#[cfg(test)]
mod tests;
