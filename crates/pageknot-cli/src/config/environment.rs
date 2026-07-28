use std::collections::BTreeMap;

use pageknot::{
    BrowserChannel, BrowserInstallationPolicy, CaptureScope, ColorScheme, ConfigProvenance,
    Milliseconds, MissingResourcePolicy, ReadinessMode, Result, VerificationPolicy, Viewport,
};
use serde_json::Value;
use url::Url;

use super::{
    ConfigScalar, ProfilePatch, ResolvedConfig, SimpleNetworkPolicy, apply_profile_patch,
    apply_secret_paths, config_value_error, parse_duration, record,
};

pub(super) fn apply_environment(
    resolved: &mut ResolvedConfig,
    environment: &BTreeMap<String, String>,
) -> Result<()> {
    let provenance = ConfigProvenance::Environment;
    if let Some(value) = environment.get("PAGEKNOT_BROWSER_PATH") {
        set_browser_path(resolved, value.clone(), provenance);
    }
    if let Some(value) = environment.get("PAGEKNOT_CDP_URL") {
        set_cdp_url(resolved, parse_cdp_url(value)?, provenance);
    }
    if let Some(value) = environment.get("PAGEKNOT_CACHE_DIR") {
        resolved.cache_dir = Some(value.clone());
        record(
            resolved,
            "browser.cacheDir",
            Value::String(value.clone()),
            provenance,
            false,
        );
    }
    if let Some(value) = environment.get("PAGEKNOT_BROWSER_CHANNEL") {
        resolved.browser_channel = match value.as_str() {
            "auto" => BrowserChannel::Auto,
            "managed" => BrowserChannel::Managed,
            "system" => BrowserChannel::System,
            _ => return Err(config_value_error("PAGEKNOT_BROWSER_CHANNEL", value)),
        };
        record(
            resolved,
            "browser.channel",
            serde_json::to_value(resolved.browser_channel).unwrap_or(Value::Null),
            provenance,
            false,
        );
    }
    if let Some(value) = environment.get("PAGEKNOT_BROWSER_INSTALLATION") {
        resolved.browser_installation = match value.as_str() {
            "explicit" => BrowserInstallationPolicy::Explicit,
            "install-managed" => BrowserInstallationPolicy::InstallManaged,
            _ => {
                return Err(config_value_error("PAGEKNOT_BROWSER_INSTALLATION", value));
            }
        };
        record(
            resolved,
            "browser.installation",
            serde_json::to_value(resolved.browser_installation).unwrap_or(Value::Null),
            provenance,
            false,
        );
    }
    if let Some(value) = environment.get("PAGEKNOT_HEADLESS") {
        resolved.headless = parse_bool(value)?;
        record(
            resolved,
            "browser.headless",
            Value::Bool(resolved.headless),
            provenance,
            false,
        );
    }

    let mut profile = ProfilePatch::default();
    if let Some(value) = environment.get("PAGEKNOT_VIEWPORT") {
        profile.environment.viewport = Some(parse_viewport(value)?);
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
        let duration = parse_duration(&ConfigScalar::String(value.clone()))?;
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
        let duration = parse_duration(&ConfigScalar::String(value.clone()))?;
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
    apply_secret_paths(
        resolved,
        environment.get("PAGEKNOT_HEADERS").cloned(),
        environment.get("PAGEKNOT_COOKIES").cloned(),
        provenance,
    );
    Ok(())
}

pub(super) fn set_browser_path(
    resolved: &mut ResolvedConfig,
    path: String,
    provenance: ConfigProvenance,
) {
    resolved.browser_path = Some(path.clone());
    resolved.cdp_url = None;
    resolved.configuration.remove("browser.cdpUrl");
    record(
        resolved,
        "browser.path",
        Value::String(path),
        provenance,
        false,
    );
}

pub(super) fn set_cdp_url(
    resolved: &mut ResolvedConfig,
    endpoint: Url,
    provenance: ConfigProvenance,
) {
    resolved.cdp_url = Some(endpoint);
    resolved.browser_path = None;
    resolved.configuration.remove("browser.path");
    record(
        resolved,
        "browser.cdpUrl",
        Value::String("[redacted endpoint]".to_owned()),
        provenance,
        true,
    );
}

pub(super) fn parse_cdp_url(value: &str) -> Result<Url> {
    let endpoint =
        Url::parse(value).map_err(|_| config_value_error("remote browser endpoint", value))?;
    if matches!(endpoint.scheme(), "http" | "https" | "ws" | "wss") {
        Ok(endpoint)
    } else {
        Err(config_value_error("remote browser endpoint", value))
    }
}

fn parse_bool(value: &str) -> Result<bool> {
    match value {
        "1" | "true" | "yes" => Ok(true),
        "0" | "false" | "no" => Ok(false),
        _ => Err(config_value_error("boolean", value)),
    }
}

fn parse_viewport(value: &str) -> Result<Viewport> {
    let (width, height) = value
        .split_once(['x', 'X'])
        .ok_or_else(|| config_value_error("viewport", value))?;
    let width = width
        .parse::<u32>()
        .map_err(|_| config_value_error("viewport", value))?;
    let height = height
        .parse::<u32>()
        .map_err(|_| config_value_error("viewport", value))?;
    Ok(Viewport {
        width,
        height,
        scale: 1,
    })
}
