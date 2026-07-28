use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use directories::ProjectDirs;
use pageknot::{
    BrowserChannel, BrowserInstallationPolicy, BrowserSpec, CaptureProfile, CaptureRequest,
    CaptureScope, ColorScheme, ConfigProvenance, EffectiveConfigValue, LazyLoadPolicy,
    Milliseconds, MissingResourcePolicy, NetworkPolicy, PageKnotBuilder, PageKnotError,
    ReadinessMode, ReducedMotion, Result, VerificationPolicy, Viewport, ViewportSweepPolicy,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use url::Url;

mod environment;

use environment::{apply_environment, parse_cdp_url, set_browser_path, set_cdp_url};

const MAXIMUM_CONFIG_BYTES: u64 = 1024 * 1024;

#[derive(Clone, Debug)]
pub(crate) struct ResolvedConfig {
    browser_path: Option<String>,
    cdp_url: Option<Url>,
    cache_dir: Option<String>,
    browser_channel: BrowserChannel,
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
        let user_path = user_config_path();
        let user = match user_path.as_deref() {
            Some(path) if path.is_file() => read_config(path)?,
            Some(_) | None => ConfigFile::default(),
        };
        let explicit_path = explicit_path
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("PAGEKNOT_CONFIG").map(PathBuf::from));
        let explicit = match explicit_path.as_deref() {
            Some(path) => read_config(path)?,
            None => ConfigFile::default(),
        };
        let environment = std::env::vars()
            .filter(|(name, _)| name.starts_with("PAGEKNOT_"))
            .collect::<BTreeMap<_, _>>();
        resolve_documents(user, explicit, &environment, selected_profile)
    }

    pub(crate) fn apply_to_request(&self, request: &mut CaptureRequest) {
        self.profile.apply_to(request);
        request.browser = if let Some(path) = &self.browser_path {
            BrowserSpec::Executable(path.as_str().into())
        } else if let Some(endpoint) = &self.cdp_url {
            BrowserSpec::Remote(endpoint.clone())
        } else {
            BrowserSpec::Auto
        };
    }

    pub(crate) fn apply_to_builder(&self, mut builder: PageKnotBuilder) -> PageKnotBuilder {
        builder = builder
            .headed(!self.headless)
            .browser_channel(self.browser_channel)
            .browser_installation(self.browser_installation)
            .maximum_contexts(self.maximum_contexts)
            .effective_configuration(self.configuration.values().cloned().collect())
            .default_network_policy(self.profile.network.clone());
        if let Some(path) = &self.cache_dir {
            builder = builder.cache_dir(path);
        }
        if let Some(path) = &self.browser_path {
            builder = builder.browser_path(path);
        } else if let Some(endpoint) = &self.cdp_url {
            builder = builder.cdp_url(endpoint.clone());
        }
        builder
    }

    pub(crate) fn apply_browser_path_flag(&mut self, path: String) {
        set_browser_path(self, path, ConfigProvenance::Flag);
    }

    pub(crate) fn apply_cdp_url_flag(&mut self, value: &str) -> Result<()> {
        let endpoint = parse_cdp_url(value)?;
        set_cdp_url(self, endpoint, ConfigProvenance::Flag);
        Ok(())
    }

    pub(crate) const fn uses_remote_browser(&self) -> bool {
        self.cdp_url.is_some()
    }

    pub(crate) fn headers_path(&self) -> Option<&str> {
        self.headers_path.as_deref()
    }

    pub(crate) fn cookies_path(&self) -> Option<&str> {
        self.cookies_path.as_deref()
    }
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct ConfigFile {
    default_profile: Option<String>,
    browser: BrowserConfig,
    profile: BTreeMap<String, ProfilePatch>,
    headers: Option<String>,
    cookies: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct BrowserConfig {
    channel: Option<BrowserChannel>,
    installation: Option<BrowserInstallationPolicy>,
    path: Option<String>,
    cdp_url: Option<String>,
    cache_dir: Option<String>,
    headless: Option<bool>,
    maximum_contexts: Option<u16>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct ProfilePatch {
    verification: Option<VerificationPolicy>,
    missing_resources: Option<MissingResourcePolicy>,
    network_policy: Option<SimpleNetworkPolicy>,
    preserve_password_values: Option<bool>,
    scope: Option<CaptureScope>,
    selector: Option<String>,
    allowed_file_roots: Option<Vec<String>>,
    optimizations: OptimizationConfig,
    environment: EnvironmentConfig,
    readiness: ReadinessConfig,
    limits: LimitsConfig,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct OptimizationConfig {
    remove_unused_css: Option<bool>,
    remove_unused_fonts: Option<bool>,
    remove_hidden_elements: Option<bool>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum SimpleNetworkPolicy {
    Standard,
    Server,
    Unrestricted,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct EnvironmentConfig {
    viewport: Option<Viewport>,
    locale: Option<String>,
    timezone: Option<String>,
    color_scheme: Option<ColorScheme>,
    reduced_motion: Option<ReducedMotion>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct ReadinessConfig {
    mode: Option<ReadinessMode>,
    network_quiet: Option<ConfigScalar>,
    mutation_quiet: Option<ConfigScalar>,
    delay: Option<ConfigScalar>,
    lazy_load: Option<LazyLoadName>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "kebab-case")]
enum LazyLoadName {
    Disabled,
    ViewportSweep,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct LimitsConfig {
    duration: Option<ConfigScalar>,
    redirects: Option<u32>,
    frames: Option<u32>,
    nodes: Option<u64>,
    resources: Option<u32>,
    resource_bytes: Option<ConfigScalar>,
    total_resource_bytes: Option<ConfigScalar>,
    collector_chunk_bytes: Option<ConfigScalar>,
    concurrent_resources: Option<u16>,
    artifact_bytes: Option<ConfigScalar>,
    css_import_depth: Option<u16>,
    frame_depth: Option<u16>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(untagged)]
enum ConfigScalar {
    Integer(u64),
    String(String),
}

fn resolve_documents(
    user: ConfigFile,
    explicit: ConfigFile,
    environment: &BTreeMap<String, String>,
    selected_profile: Option<&str>,
) -> Result<ResolvedConfig> {
    let (profile, profile_provenance) = if let Some(profile) = selected_profile {
        (profile.to_owned(), ConfigProvenance::Flag)
    } else if let Some(profile) = environment.get("PAGEKNOT_PROFILE") {
        (profile.clone(), ConfigProvenance::Environment)
    } else if let Some(profile) = explicit.default_profile.clone() {
        (profile, ConfigProvenance::ExplicitConfig)
    } else if let Some(profile) = user.default_profile.clone() {
        (profile, ConfigProvenance::UserConfig)
    } else {
        ("default".to_owned(), ConfigProvenance::Default)
    };
    let user_profile = user.profile.get(&profile);
    let explicit_profile = explicit.profile.get(&profile);
    let built_in = CaptureProfile::named(&profile).ok();
    if built_in.is_none() && user_profile.is_none() && explicit_profile.is_none() {
        return Err(config_error(
            "pageknot.config.profile",
            format!("configuration profile `{profile}` is undefined"),
        ));
    }
    let mut resolved = default_config(profile, profile_provenance);
    if let Some(profile) = built_in {
        apply_builtin_profile(&mut resolved, profile);
    }
    if let Some(profile) = user_profile {
        apply_profile_patch(&mut resolved, profile, ConfigProvenance::Profile)?;
    }
    if let Some(profile) = explicit_profile {
        apply_profile_patch(&mut resolved, profile, ConfigProvenance::Profile)?;
    }
    apply_browser(&mut resolved, &user.browser, ConfigProvenance::UserConfig)?;
    apply_secret_paths(
        &mut resolved,
        user.headers,
        user.cookies,
        ConfigProvenance::UserConfig,
    );
    apply_browser(
        &mut resolved,
        &explicit.browser,
        ConfigProvenance::ExplicitConfig,
    )?;
    apply_secret_paths(
        &mut resolved,
        explicit.headers,
        explicit.cookies,
        ConfigProvenance::ExplicitConfig,
    );
    apply_environment(&mut resolved, environment)?;
    validate_resolved(&resolved)?;
    Ok(resolved)
}

fn default_config(profile: String, profile_provenance: ConfigProvenance) -> ResolvedConfig {
    let mut resolved = ResolvedConfig {
        browser_path: None,
        cdp_url: None,
        cache_dir: None,
        browser_channel: BrowserChannel::Auto,
        browser_installation: BrowserInstallationPolicy::InstallManaged,
        headless: true,
        maximum_contexts: 4,
        profile: CaptureProfile::default(),
        headers_path: None,
        cookies_path: None,
        configuration: BTreeMap::new(),
    };
    record(
        &mut resolved,
        "profile",
        Value::String(profile),
        profile_provenance,
        false,
    );
    record(
        &mut resolved,
        "browser.channel",
        Value::String("auto".to_owned()),
        ConfigProvenance::Default,
        false,
    );
    record(
        &mut resolved,
        "browser.installation",
        Value::String("install-managed".to_owned()),
        ConfigProvenance::Default,
        false,
    );
    record(
        &mut resolved,
        "browser.headless",
        Value::Bool(true),
        ConfigProvenance::Default,
        false,
    );
    let network = network_policy_name(&resolved.profile.network);
    record(
        &mut resolved,
        "networkPolicy",
        Value::String(network.to_owned()),
        ConfigProvenance::Default,
        false,
    );
    resolved
}

fn apply_builtin_profile(resolved: &mut ResolvedConfig, profile: CaptureProfile) {
    let defaults = CaptureProfile::default();
    resolved.profile = profile;
    if resolved.profile.capture.missing_resources != defaults.capture.missing_resources {
        let value =
            serde_json::to_value(resolved.profile.capture.missing_resources).unwrap_or(Value::Null);
        record(
            resolved,
            "missingResources",
            value,
            ConfigProvenance::Profile,
            false,
        );
    }
    if resolved.profile.network != defaults.network {
        let value = network_policy_name(&resolved.profile.network);
        record(
            resolved,
            "networkPolicy",
            Value::String(value.to_owned()),
            ConfigProvenance::Profile,
            false,
        );
    }
}

fn apply_profile_patch(
    resolved: &mut ResolvedConfig,
    profile: &ProfilePatch,
    provenance: ConfigProvenance,
) -> Result<()> {
    if let Some(verification) = profile.verification {
        resolved.profile.verification = verification;
        record(
            resolved,
            "verification",
            serde_json::to_value(verification).unwrap_or(Value::Null),
            provenance,
            false,
        );
    }
    if let Some(missing) = profile.missing_resources {
        resolved.profile.capture.missing_resources = missing;
        record(
            resolved,
            "missingResources",
            serde_json::to_value(missing).unwrap_or(Value::Null),
            provenance,
            false,
        );
    }
    if let Some(network) = profile.network_policy {
        resolved.profile.network = simple_network_policy(network);
        record(
            resolved,
            "networkPolicy",
            serde_json::to_value(network).unwrap_or(Value::Null),
            provenance,
            false,
        );
    }
    if let Some(preserve) = profile.preserve_password_values {
        resolved.profile.capture.preserve_password_values = preserve;
        record(
            resolved,
            "preservePasswordValues",
            Value::Bool(preserve),
            provenance,
            false,
        );
    }
    if let Some(scope) = profile.scope {
        resolved.profile.capture.scope = scope;
        resolved.profile.capture.selector = None;
        record(
            resolved,
            "scope",
            serde_json::to_value(scope).unwrap_or(Value::Null),
            provenance,
            false,
        );
    }
    if let Some(selector) = &profile.selector {
        resolved.profile.capture.scope = CaptureScope::Page;
        resolved.profile.capture.selector = Some(selector.clone());
        record(
            resolved,
            "selector",
            Value::String(selector.clone()),
            provenance,
            false,
        );
    }
    apply_optimizations(resolved, &profile.optimizations, provenance);
    if let Some(roots) = &profile.allowed_file_roots {
        resolved.profile.capture.allowed_file_roots =
            roots.iter().map(|root| root.as_str().into()).collect();
        record(
            resolved,
            "allowedFileRoots",
            Value::Array(roots.iter().cloned().map(Value::String).collect()),
            provenance,
            false,
        );
    }
    apply_profile_environment(resolved, &profile.environment, provenance);
    apply_readiness(resolved, &profile.readiness, provenance)?;
    apply_limits(resolved, &profile.limits, provenance)
}

fn apply_optimizations(
    resolved: &mut ResolvedConfig,
    optimizations: &OptimizationConfig,
    provenance: ConfigProvenance,
) {
    if let Some(value) = optimizations.remove_unused_css {
        resolved.profile.capture.optimizations.remove_unused_css = value;
        record(
            resolved,
            "optimizations.removeUnusedCss",
            Value::Bool(value),
            provenance,
            false,
        );
    }
    if let Some(value) = optimizations.remove_unused_fonts {
        resolved.profile.capture.optimizations.remove_unused_fonts = value;
        record(
            resolved,
            "optimizations.removeUnusedFonts",
            Value::Bool(value),
            provenance,
            false,
        );
    }
    if let Some(value) = optimizations.remove_hidden_elements {
        resolved
            .profile
            .capture
            .optimizations
            .remove_hidden_elements = value;
        record(
            resolved,
            "optimizations.removeHiddenElements",
            Value::Bool(value),
            provenance,
            false,
        );
    }
}

fn apply_profile_environment(
    resolved: &mut ResolvedConfig,
    environment: &EnvironmentConfig,
    provenance: ConfigProvenance,
) {
    if let Some(viewport) = environment.viewport {
        resolved.profile.environment.viewport = viewport;
        record(
            resolved,
            "environment.viewport",
            serde_json::to_value(viewport).unwrap_or(Value::Null),
            provenance,
            false,
        );
    }
    if let Some(locale) = &environment.locale {
        resolved.profile.environment.locale = locale.clone();
        record(
            resolved,
            "environment.locale",
            Value::String(locale.clone()),
            provenance,
            false,
        );
    }
    if let Some(timezone) = &environment.timezone {
        resolved.profile.environment.timezone = timezone.clone();
        record(
            resolved,
            "environment.timezone",
            Value::String(timezone.clone()),
            provenance,
            false,
        );
    }
    if let Some(color_scheme) = environment.color_scheme {
        resolved.profile.environment.color_scheme = color_scheme;
        record(
            resolved,
            "environment.colorScheme",
            serde_json::to_value(color_scheme).unwrap_or(Value::Null),
            provenance,
            false,
        );
    }
    if let Some(reduced_motion) = environment.reduced_motion {
        resolved.profile.environment.reduced_motion = reduced_motion;
        record(
            resolved,
            "environment.reducedMotion",
            serde_json::to_value(reduced_motion).unwrap_or(Value::Null),
            provenance,
            false,
        );
    }
}

fn apply_readiness(
    resolved: &mut ResolvedConfig,
    readiness: &ReadinessConfig,
    provenance: ConfigProvenance,
) -> Result<()> {
    if let Some(mode) = readiness.mode {
        resolved.profile.readiness.mode = mode;
        record(
            resolved,
            "readiness.mode",
            serde_json::to_value(mode).unwrap_or(Value::Null),
            provenance,
            false,
        );
    }
    if let Some(value) = &readiness.network_quiet {
        resolved.profile.readiness.network_quiet = Milliseconds::from(parse_duration(value)?);
        let milliseconds = resolved.profile.readiness.network_quiet.get();
        record(
            resolved,
            "readiness.networkQuiet",
            Value::from(milliseconds),
            provenance,
            false,
        );
    }
    if let Some(value) = &readiness.mutation_quiet {
        resolved.profile.readiness.mutation_quiet = Milliseconds::from(parse_duration(value)?);
        let milliseconds = resolved.profile.readiness.mutation_quiet.get();
        record(
            resolved,
            "readiness.mutationQuiet",
            Value::from(milliseconds),
            provenance,
            false,
        );
    }
    if let Some(value) = &readiness.delay {
        resolved.profile.readiness.delay = Milliseconds::from(parse_duration(value)?);
        let milliseconds = resolved.profile.readiness.delay.get();
        record(
            resolved,
            "readiness.delay",
            Value::from(milliseconds),
            provenance,
            false,
        );
    }
    if let Some(lazy_load) = readiness.lazy_load {
        resolved.profile.readiness.lazy_load = match lazy_load {
            LazyLoadName::Disabled => LazyLoadPolicy::Disabled,
            LazyLoadName::ViewportSweep => {
                LazyLoadPolicy::ViewportSweep(ViewportSweepPolicy::default())
            }
        };
        let value = match lazy_load {
            LazyLoadName::Disabled => "disabled",
            LazyLoadName::ViewportSweep => "viewport-sweep",
        };
        record(
            resolved,
            "readiness.lazyLoad",
            Value::String(value.to_owned()),
            provenance,
            false,
        );
    }
    Ok(())
}

fn apply_limits(
    resolved: &mut ResolvedConfig,
    limits: &LimitsConfig,
    provenance: ConfigProvenance,
) -> Result<()> {
    if let Some(value) = &limits.duration {
        resolved.profile.limits.duration = Milliseconds::from(parse_duration(value)?);
        let milliseconds = resolved.profile.limits.duration.get();
        record(
            resolved,
            "limits.duration",
            Value::from(milliseconds),
            provenance,
            false,
        );
    }
    macro_rules! set_limit {
        ($field:ident, $name:literal) => {
            if let Some(value) = limits.$field {
                resolved.profile.limits.$field = value;
                record(
                    resolved,
                    concat!("limits.", $name),
                    Value::from(value),
                    provenance,
                    false,
                );
            }
        };
    }
    set_limit!(redirects, "redirects");
    set_limit!(frames, "frames");
    set_limit!(nodes, "nodes");
    set_limit!(resources, "resources");
    set_limit!(concurrent_resources, "concurrentResources");
    set_limit!(css_import_depth, "cssImportDepth");
    set_limit!(frame_depth, "frameDepth");
    for (value, field) in [
        (&limits.resource_bytes, "resourceBytes"),
        (&limits.total_resource_bytes, "totalResourceBytes"),
        (&limits.collector_chunk_bytes, "collectorChunkBytes"),
        (&limits.artifact_bytes, "artifactBytes"),
    ] {
        if let Some(value) = value {
            let bytes = parse_bytes(value)?;
            match field {
                "resourceBytes" => resolved.profile.limits.resource_bytes = bytes,
                "totalResourceBytes" => resolved.profile.limits.total_resource_bytes = bytes,
                "collectorChunkBytes" => resolved.profile.limits.collector_chunk_bytes = bytes,
                "artifactBytes" => resolved.profile.limits.artifact_bytes = bytes,
                _ => {}
            }
            record(
                resolved,
                &format!("limits.{field}"),
                Value::from(bytes),
                provenance,
                false,
            );
        }
    }
    Ok(())
}

fn apply_browser(
    resolved: &mut ResolvedConfig,
    browser: &BrowserConfig,
    provenance: ConfigProvenance,
) -> Result<()> {
    if let Some(channel) = browser.channel {
        resolved.browser_channel = channel;
        record(
            resolved,
            "browser.channel",
            serde_json::to_value(channel).unwrap_or(Value::Null),
            provenance,
            false,
        );
    }
    if let Some(installation) = browser.installation {
        resolved.browser_installation = installation;
        record(
            resolved,
            "browser.installation",
            serde_json::to_value(installation).unwrap_or(Value::Null),
            provenance,
            false,
        );
    }
    if let Some(path) = &browser.path {
        set_browser_path(resolved, path.clone(), provenance);
    }
    if let Some(endpoint) = &browser.cdp_url {
        set_cdp_url(resolved, parse_cdp_url(endpoint)?, provenance);
    }
    if let Some(cache_dir) = &browser.cache_dir {
        resolved.cache_dir = Some(cache_dir.clone());
        record(
            resolved,
            "browser.cacheDir",
            Value::String(cache_dir.clone()),
            provenance,
            false,
        );
    }
    if let Some(headless) = browser.headless {
        resolved.headless = headless;
        record(
            resolved,
            "browser.headless",
            Value::Bool(headless),
            provenance,
            false,
        );
    }
    if let Some(maximum_contexts) = browser.maximum_contexts {
        resolved.maximum_contexts = maximum_contexts;
        record(
            resolved,
            "browser.maximumContexts",
            Value::from(maximum_contexts),
            provenance,
            false,
        );
    }
    Ok(())
}

fn apply_secret_paths(
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

fn validate_resolved(resolved: &ResolvedConfig) -> Result<()> {
    if resolved.maximum_contexts == 0 {
        return Err(config_error(
            "pageknot.config.value",
            "browser.maximum_contexts must be greater than zero",
        ));
    }
    if resolved.browser_path.is_some() && resolved.cdp_url.is_some() {
        return Err(config_error(
            "pageknot.config.value",
            "browser path and remote endpoint are mutually exclusive",
        ));
    }
    let mut request = CaptureRequest::builder("https://example.invalid")?.build()?;
    resolved.apply_to_request(&mut request);
    // Command flags finalize browser selection before capture validation.
    // Automatic selection keeps remote policy checks on the effective request.
    request.browser = BrowserSpec::Auto;
    request.validate()
}

fn read_config(path: &Path) -> Result<ConfigFile> {
    let metadata = std::fs::metadata(path).map_err(|error| {
        config_error(
            "pageknot.config.read",
            format!(
                "failed to inspect configuration file `{}`: {error}",
                path.display()
            ),
        )
    })?;
    if !metadata.is_file() || metadata.len() > MAXIMUM_CONFIG_BYTES {
        return Err(config_error(
            "pageknot.config.read",
            format!(
                "configuration file `{}` is not a bounded regular file",
                path.display()
            ),
        ));
    }
    let source = std::fs::read_to_string(path).map_err(|error| {
        config_error(
            "pageknot.config.read",
            format!(
                "failed to read configuration file `{}`: {error}",
                path.display()
            ),
        )
    })?;
    toml::from_str(&source).map_err(|error| {
        config_error(
            "pageknot.config.parse",
            format!(
                "configuration file `{}` is invalid: {error}",
                path.display()
            ),
        )
    })
}

fn user_config_path() -> Option<PathBuf> {
    ProjectDirs::from("", "", "pageknot")
        .map(|directories| directories.config_dir().join("config.toml"))
}

fn parse_duration(value: &ConfigScalar) -> Result<Duration> {
    match value {
        ConfigScalar::Integer(milliseconds) => Ok(Duration::from_millis(*milliseconds)),
        ConfigScalar::String(value) => {
            let split = value
                .find(|character: char| !character.is_ascii_digit() && character != '.')
                .unwrap_or(value.len());
            let (number, unit) = value.split_at(split);
            let number = number
                .parse::<f64>()
                .map_err(|_| config_value_error("duration", value))?;
            if !number.is_finite() || number <= 0.0 {
                return Err(config_value_error("duration", value));
            }
            let seconds = match unit {
                "ms" => number / 1000.0,
                "" | "s" => number,
                "m" => number * 60.0,
                "h" => number * 3600.0,
                _ => return Err(config_value_error("duration", value)),
            };
            Ok(Duration::from_secs_f64(seconds))
        }
    }
}

fn parse_bytes(value: &ConfigScalar) -> Result<u64> {
    let value = match value {
        ConfigScalar::Integer(value) => return Ok(*value),
        ConfigScalar::String(value) => value,
    };
    let split = value
        .find(|character: char| !character.is_ascii_digit())
        .unwrap_or(value.len());
    let (number, unit) = value.split_at(split);
    let number = number
        .parse::<u64>()
        .map_err(|_| config_value_error("byte size", value))?;
    let multiplier = match unit.to_ascii_lowercase().as_str() {
        "" | "b" => 1,
        "kib" => 1024,
        "mib" => 1024 * 1024,
        "gib" => 1024 * 1024 * 1024,
        _ => return Err(config_value_error("byte size", value)),
    };
    number
        .checked_mul(multiplier)
        .ok_or_else(|| config_value_error("byte size", value))
}

fn simple_network_policy(value: SimpleNetworkPolicy) -> NetworkPolicy {
    match value {
        SimpleNetworkPolicy::Standard => NetworkPolicy::Standard,
        SimpleNetworkPolicy::Server => NetworkPolicy::Server,
        SimpleNetworkPolicy::Unrestricted => NetworkPolicy::Unrestricted,
    }
}

fn network_policy_name(value: &NetworkPolicy) -> &'static str {
    match value {
        NetworkPolicy::Standard => "standard",
        NetworkPolicy::Server => "server",
        NetworkPolicy::Unrestricted => "unrestricted",
        NetworkPolicy::Custom(_) => "custom",
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

fn config_value_error(name: &str, value: &str) -> PageKnotError {
    config_error(
        "pageknot.config.value",
        format!("configuration {name} value `{value}` is invalid"),
    )
}

fn config_error(code: &'static str, message: impl Into<String>) -> PageKnotError {
    PageKnotError::new(code, pageknot::ErrorStage::Validation, message)
}

#[cfg(test)]
mod tests;
