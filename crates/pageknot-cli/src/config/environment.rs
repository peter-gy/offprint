use std::collections::BTreeMap;

use pageknot::{
    BrowserChannel, BrowserInstallationPolicy, CaptureScope, ColorScheme, ConfigProvenance,
    Milliseconds, MissingResourcePolicy, ReadinessMode, Result, VerificationPolicy,
};

use super::browser::apply_browser;
use super::document::{BrowserConfig, ConfigScalar, ProfilePatch, SimpleNetworkPolicy};
use super::profile::apply_profile_patch;
use super::value::{parse_config_duration, parse_viewport_text};
use super::{ResolvedConfig, config_value_error, set_secret_paths};

pub(super) fn apply_environment(
    resolved: &mut ResolvedConfig,
    environment: &BTreeMap<String, String>,
) -> Result<()> {
    let provenance = ConfigProvenance::Environment;
    let mut browser = BrowserConfig {
        path: environment.get("PAGEKNOT_BROWSER_PATH").cloned(),
        cdp_url: environment.get("PAGEKNOT_CDP_URL").cloned(),
        cache_dir: environment.get("PAGEKNOT_CACHE_DIR").cloned(),
        ..BrowserConfig::default()
    };
    if let Some(value) = environment.get("PAGEKNOT_BROWSER_CHANNEL") {
        browser.channel = Some(match value.as_str() {
            "auto" => BrowserChannel::Auto,
            "managed" => BrowserChannel::Managed,
            "system" => BrowserChannel::System,
            _ => return Err(config_value_error("PAGEKNOT_BROWSER_CHANNEL", value)),
        });
    }
    if let Some(value) = environment.get("PAGEKNOT_BROWSER_INSTALLATION") {
        browser.installation = Some(match value.as_str() {
            "explicit" => BrowserInstallationPolicy::Explicit,
            "install-managed" => BrowserInstallationPolicy::InstallManaged,
            _ => {
                return Err(config_value_error("PAGEKNOT_BROWSER_INSTALLATION", value));
            }
        });
    }
    if let Some(value) = environment.get("PAGEKNOT_HEADLESS") {
        browser.headless = Some(parse_bool(value)?);
    }
    apply_browser(resolved, &browser, provenance)?;

    let mut profile = ProfilePatch::default();
    if let Some(value) = environment.get("PAGEKNOT_VIEWPORT") {
        profile.environment.viewport =
            Some(parse_viewport_text(value).ok_or_else(|| config_value_error("viewport", value))?);
    }
    if let Some(value) = environment.get("PAGEKNOT_LOCALE") {
        profile.environment.locale = Some(value.clone());
    }
    if let Some(value) = environment.get("PAGEKNOT_TIMEZONE") {
        profile.environment.timezone = Some(value.clone());
    }
    if let Some(value) = environment.get("PAGEKNOT_COLOR_SCHEME") {
        profile.environment.color_scheme = Some(match value.as_str() {
            "light" => ColorScheme::Light,
            "dark" => ColorScheme::Dark,
            _ => return Err(config_value_error("PAGEKNOT_COLOR_SCHEME", value)),
        });
    }
    if let Some(value) = environment.get("PAGEKNOT_TIMEOUT") {
        let duration = parse_config_duration(&ConfigScalar::String(value.clone()))?;
        profile.limits.duration = Some(ConfigScalar::Integer(Milliseconds::from(duration).get()));
    }
    if let Some(value) = environment.get("PAGEKNOT_WAIT_UNTIL") {
        profile.readiness.mode = Some(match value.as_str() {
            "render-idle" => ReadinessMode::RenderIdle,
            "network-idle" => ReadinessMode::NetworkIdle,
            "load" => ReadinessMode::Load,
            "dom-content-loaded" => ReadinessMode::DomContentLoaded,
            _ => return Err(config_value_error("PAGEKNOT_WAIT_UNTIL", value)),
        });
    }
    if let Some(value) = environment.get("PAGEKNOT_DELAY") {
        let duration = parse_config_duration(&ConfigScalar::String(value.clone()))?;
        profile.readiness.delay = Some(ConfigScalar::Integer(Milliseconds::from(duration).get()));
    }
    if let Some(value) = environment.get("PAGEKNOT_MISSING_RESOURCES") {
        profile.missing_resources = Some(match value.as_str() {
            "warn" => MissingResourcePolicy::Warn,
            "fail" => MissingResourcePolicy::Fail,
            _ => return Err(config_value_error("PAGEKNOT_MISSING_RESOURCES", value)),
        });
    }
    if let Some(value) = environment.get("PAGEKNOT_SCOPE") {
        profile.scope = Some(match value.as_str() {
            "page" => CaptureScope::Page,
            "selection" => CaptureScope::Selection,
            _ => return Err(config_value_error("PAGEKNOT_SCOPE", value)),
        });
    }
    if let Some(value) = environment.get("PAGEKNOT_SELECTOR") {
        profile.selector = Some(value.clone());
    }
    if let Some(value) = environment.get("PAGEKNOT_REMOVE_UNUSED_CSS") {
        profile.optimizations.remove_unused_css = Some(parse_bool(value)?);
    }
    if let Some(value) = environment.get("PAGEKNOT_REMOVE_UNUSED_FONTS") {
        profile.optimizations.remove_unused_fonts = Some(parse_bool(value)?);
    }
    if let Some(value) = environment.get("PAGEKNOT_REMOVE_HIDDEN_ELEMENTS") {
        profile.optimizations.remove_hidden_elements = Some(parse_bool(value)?);
    }
    if let Some(value) = environment.get("PAGEKNOT_VERIFY") {
        profile.verification = Some(match value.as_str() {
            "static" => VerificationPolicy::Static,
            "offline" => VerificationPolicy::Offline,
            _ => return Err(config_value_error("PAGEKNOT_VERIFY", value)),
        });
    }
    if let Some(value) = environment.get("PAGEKNOT_NETWORK_POLICY") {
        profile.network_policy = Some(match value.as_str() {
            "standard" => SimpleNetworkPolicy::Standard,
            "server" => SimpleNetworkPolicy::Server,
            "unrestricted" => SimpleNetworkPolicy::Unrestricted,
            _ => return Err(config_value_error("PAGEKNOT_NETWORK_POLICY", value)),
        });
    }

    apply_profile_patch(resolved, &profile, provenance)?;
    set_secret_paths(
        resolved,
        environment.get("PAGEKNOT_HEADERS").cloned(),
        environment.get("PAGEKNOT_COOKIES").cloned(),
        provenance,
    );
    Ok(())
}

fn parse_bool(value: &str) -> Result<bool> {
    match value {
        "1" | "true" | "yes" => Ok(true),
        "0" | "false" | "no" => Ok(false),
        _ => Err(config_value_error("boolean", value)),
    }
}
