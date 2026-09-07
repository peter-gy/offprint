use offprint::{BrowserSpec, ConfigProvenance, Result};
use serde_json::Value;
use url::Url;

use super::document::BrowserConfig;
use super::value::parse_cdp_endpoint;
use super::{BrowserSelection, ResolvedConfig, config_error, record};

pub(super) fn apply_browser(
    resolved: &mut ResolvedConfig,
    browser: &BrowserConfig,
    provenance: ConfigProvenance,
) -> Result<()> {
    let selection = selection_from_values(browser.path.as_deref(), browser.cdp_url.as_deref())?;
    if let Some(source) = browser.source {
        resolved.browser_source = source;
        record(
            resolved,
            "browser.source",
            serde_json::to_value(source).unwrap_or(Value::Null),
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
    if let Some(selection) = selection {
        set_browser_selection(resolved, selection, provenance);
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

pub(super) fn selection_from_values(
    path: Option<&str>,
    endpoint: Option<&str>,
) -> Result<Option<BrowserSelection>> {
    match (path, endpoint) {
        (Some(_), Some(_)) => Err(config_error(
            "offprint.config.value",
            "browser path and remote endpoint are mutually exclusive",
        )),
        (Some(path), None) => Ok(Some(BrowserSelection::Executable(path.to_owned()))),
        (None, Some(endpoint)) => parse_cdp_url(endpoint)
            .map(BrowserSelection::Remote)
            .map(Some),
        (None, None) => Ok(None),
    }
}

pub(super) fn set_browser_selection(
    resolved: &mut ResolvedConfig,
    selection: BrowserSelection,
    provenance: ConfigProvenance,
) {
    resolved.configuration.remove("browser.path");
    resolved.configuration.remove("browser.cdpUrl");
    match &selection {
        BrowserSelection::Auto => {}
        BrowserSelection::Executable(path) => record(
            resolved,
            "browser.path",
            Value::String(path.clone()),
            provenance,
            false,
        ),
        BrowserSelection::Remote(_) => record(
            resolved,
            "browser.cdpUrl",
            Value::String("[redacted endpoint]".to_owned()),
            provenance,
            true,
        ),
    }
    resolved.browser = selection;
}

pub(super) fn parse_cdp_url(value: &str) -> Result<Url> {
    parse_cdp_endpoint(value).map_err(|_| {
        config_error(
            "offprint.config.value",
            "configuration remote browser endpoint is invalid",
        )
    })
}

pub(super) fn browser_spec(selection: &BrowserSelection) -> BrowserSpec {
    match selection {
        BrowserSelection::Auto => BrowserSpec::Auto,
        BrowserSelection::Executable(path) => BrowserSpec::Executable(path.as_str().into()),
        BrowserSelection::Remote(endpoint) => BrowserSpec::Remote(endpoint.clone()),
    }
}
