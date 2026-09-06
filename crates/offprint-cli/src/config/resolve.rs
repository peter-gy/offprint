use std::collections::BTreeMap;
#[cfg(all(test, unix))]
use std::ffi::OsStr;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

use directories::ProjectDirs;
use offprint::{
    BrowserChannel, BrowserInstallationPolicy, BrowserSpec, CaptureProfile, CaptureRequest,
    ConfigProvenance, Result,
};
use serde_json::Value;

use super::browser::apply_browser;
use super::document::ConfigFile;
use super::environment::{apply_environment, is_supported_environment_name};
use super::profile::{apply_builtin_profile, apply_profile_patch, network_policy_name};
use super::{
    BrowserSelection, MAXIMUM_CONFIG_BYTES, ResolvedConfig, config_error, record, set_secret_paths,
};

pub(super) fn load(
    explicit_path: Option<&str>,
    selected_profile: Option<&str>,
) -> Result<ResolvedConfig> {
    let user_path = user_config_path();
    let user = match user_path.as_deref() {
        Some(path) if path.is_file() => read_config(path)?,
        Some(_) | None => ConfigFile::default(),
    };
    let explicit_path = explicit_path
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("OFFPRINT_CONFIG").map(PathBuf::from));
    let explicit = match explicit_path.as_deref() {
        Some(path) => read_config(path)?,
        None => ConfigFile::default(),
    };
    let environment = collect_environment(std::env::vars_os())?;
    resolve_documents(user, explicit, &environment, selected_profile)
}

pub(super) fn collect_environment(
    variables: impl IntoIterator<Item = (OsString, OsString)>,
) -> Result<BTreeMap<String, String>> {
    let mut environment = BTreeMap::new();
    for (name, value) in variables {
        let Some(name) = name.to_str() else {
            continue;
        };
        if !name.starts_with("OFFPRINT_") {
            continue;
        }
        if !is_supported_environment_name(name) {
            return Err(config_error(
                "offprint.config.field",
                format!("environment variable `{name}` is not recognized"),
            ));
        }
        let value = value.into_string().map_err(|_| {
            config_error(
                "offprint.config.value",
                format!("environment variable `{name}` must contain Unicode text"),
            )
        })?;
        environment.insert(name.to_owned(), value);
    }
    Ok(environment)
}

pub(super) fn resolve_documents(
    user: ConfigFile,
    explicit: ConfigFile,
    environment: &BTreeMap<String, String>,
    selected_profile: Option<&str>,
) -> Result<ResolvedConfig> {
    let (profile, profile_provenance) = if let Some(profile) = selected_profile {
        (profile.to_owned(), ConfigProvenance::Flag)
    } else if let Some(profile) = environment.get("OFFPRINT_PROFILE") {
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
            "offprint.config.profile",
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
    set_secret_paths(
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
    set_secret_paths(
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
        browser: BrowserSelection::Auto,
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

fn validate_resolved(resolved: &ResolvedConfig) -> Result<()> {
    if resolved.maximum_contexts == 0 {
        return Err(config_error(
            "offprint.config.value",
            "browser.maximum_contexts must be greater than zero",
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
            "offprint.config.read",
            format!(
                "failed to inspect configuration file `{}`: {error}",
                path.display()
            ),
        )
    })?;
    if !metadata.is_file() || metadata.len() > MAXIMUM_CONFIG_BYTES {
        return Err(config_error(
            "offprint.config.read",
            format!(
                "configuration file `{}` is not a bounded regular file",
                path.display()
            ),
        ));
    }
    let source = std::fs::read_to_string(path).map_err(|error| {
        config_error(
            "offprint.config.read",
            format!(
                "failed to read configuration file `{}`: {error}",
                path.display()
            ),
        )
    })?;
    toml::from_str(&source).map_err(|error| {
        config_error(
            "offprint.config.parse",
            format!(
                "configuration file `{}` is invalid: {error}",
                path.display()
            ),
        )
    })
}

fn user_config_path() -> Option<PathBuf> {
    ProjectDirs::from("", "", "offprint")
        .map(|directories| directories.config_dir().join("config.toml"))
}

#[cfg(all(test, unix))]
pub(super) fn non_unicode(value: &[u8]) -> OsString {
    use std::os::unix::ffi::OsStrExt as _;

    OsStr::from_bytes(value).to_owned()
}
