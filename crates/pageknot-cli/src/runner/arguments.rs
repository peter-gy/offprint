use std::path::Path;

use pageknot::{
    ArtifactVariant as ModelArtifactVariant, ArtifactVariantKind, CaptureRequest, CaptureScope,
    ColorScheme as ModelColorScheme, ConflictPolicy, DiagnosticsPolicy, ErrorStage,
    MarkdownOptions, Milliseconds, MissingResourcePolicy, NetworkPolicy as ModelNetworkPolicy,
    PageKnotError, PdfOptions, ReadinessMode, Result, VerificationPolicy, Viewport,
    portable_file_stem,
};
use url::Url;

use crate::command::{
    ArtifactVariant as CliArtifactVariant, CaptureArguments, ColorScheme, ConflictMode,
    ContentScope, MissingResources, NetworkPolicy, VerificationLevel, WaitUntil,
};
use crate::config::value::{
    CdpEndpointError, parse_cdp_endpoint, parse_duration_text, parse_viewport_text,
};
use crate::config::{ResolvedConfig, set_readiness_mode};

pub(super) fn require_local_browser(resolved: &ResolvedConfig, operation: &str) -> Result<()> {
    if resolved.uses_remote_browser() {
        return Err(PageKnotError::new(
            "pageknot.input.browser_selection",
            ErrorStage::Validation,
            format!(
                "{operation} requires a local Chrome or Chromium executable. Set \
                 `--browser-path` or remove `browser.cdp_url` from configuration"
            ),
        ));
    }
    Ok(())
}

pub(super) fn apply_capture_arguments(
    request: &mut CaptureRequest,
    arguments: &CaptureArguments,
) -> Result<()> {
    if arguments.headed {
        request.headed = Some(true);
    }
    if let Some(viewport) = &arguments.viewport {
        request.environment.viewport = parse_viewport(viewport)?;
    }
    if let Some(locale) = &arguments.locale {
        request.environment.locale = locale.clone();
    }
    if let Some(timezone) = &arguments.timezone {
        request.environment.timezone = timezone.clone();
    }
    if let Some(color_scheme) = arguments.color_scheme {
        request.environment.color_scheme = match color_scheme {
            ColorScheme::Light => ModelColorScheme::Light,
            ColorScheme::Dark => ModelColorScheme::Dark,
        };
    }
    if let Some(timeout) = &arguments.timeout {
        request.limits.duration = Milliseconds::from(parse_duration(timeout)?);
    }
    if let Some(wait_until) = arguments.wait_until {
        let mode = match wait_until {
            WaitUntil::RenderIdle => ReadinessMode::RenderIdle,
            WaitUntil::NetworkIdle => ReadinessMode::NetworkIdle,
            WaitUntil::Load => ReadinessMode::Load,
            WaitUntil::DomContentLoaded => ReadinessMode::DomContentLoaded,
        };
        set_readiness_mode(&mut request.readiness, mode);
    }
    if let Some(delay) = &arguments.delay {
        request.readiness.delay = Milliseconds::from(parse_duration(delay)?);
    }
    if let Some(missing) = arguments.missing_resources {
        request.capture.missing_resources = match missing {
            MissingResources::Warn => MissingResourcePolicy::Warn,
            MissingResources::Fail => MissingResourcePolicy::Fail,
        };
    }
    if let Some(scope) = arguments.scope {
        request.capture.scope = match scope {
            ContentScope::Page => CaptureScope::Page,
            ContentScope::Selection => CaptureScope::Selection,
        };
        request.capture.selector = None;
    }
    if let Some(selector) = &arguments.selector {
        request.capture.scope = CaptureScope::Page;
        request.capture.selector = Some(selector.clone());
    }
    if arguments.remove_unused_css {
        request.capture.optimizations.remove_unused_css = true;
    }
    if arguments.remove_unused_fonts {
        request.capture.optimizations.remove_unused_fonts = true;
    }
    if arguments.remove_hidden_elements {
        request.capture.optimizations.remove_hidden_elements = true;
    }
    if let Some(level) = arguments.verify {
        request.verification = verification_policy(level);
    }
    if let Some(network) = arguments.network_policy {
        request.network = match network {
            NetworkPolicy::Standard => ModelNetworkPolicy::Standard,
            NetworkPolicy::Server => ModelNetworkPolicy::Server,
            NetworkPolicy::Unrestricted => ModelNetworkPolicy::Unrestricted,
        };
    }
    if let Some(directory) = &arguments.diagnostics {
        request.diagnostics = DiagnosticsPolicy {
            directory: Some(directory.as_str().into()),
            ..DiagnosticsPolicy::default()
        };
    }
    request.validate()
}

pub(super) const fn verification_policy(level: VerificationLevel) -> VerificationPolicy {
    match level {
        VerificationLevel::Static => VerificationPolicy::Static,
        VerificationLevel::Offline => VerificationPolicy::Offline,
    }
}

pub(super) const fn artifact_variant_kind(variant: CliArtifactVariant) -> ArtifactVariantKind {
    match variant {
        CliArtifactVariant::Pdf => ArtifactVariantKind::Pdf,
        CliArtifactVariant::Markdown => ArtifactVariantKind::Markdown,
        CliArtifactVariant::Zip => ArtifactVariantKind::Zip,
        CliArtifactVariant::SelfExtracting => ArtifactVariantKind::SelfExtracting,
        CliArtifactVariant::Mhtml => ArtifactVariantKind::Mhtml,
    }
}

pub(super) const fn export_variant(
    variant: CliArtifactVariant,
    pdf: PdfOptions,
    markdown: MarkdownOptions,
) -> ModelArtifactVariant {
    match variant {
        CliArtifactVariant::Pdf => ModelArtifactVariant::Pdf(pdf),
        CliArtifactVariant::Markdown => ModelArtifactVariant::Markdown(markdown),
        CliArtifactVariant::Zip => ModelArtifactVariant::Zip,
        CliArtifactVariant::SelfExtracting => ModelArtifactVariant::SelfExtracting,
        CliArtifactVariant::Mhtml => ModelArtifactVariant::Mhtml,
    }
}

pub(super) const fn conflict_policy(mode: ConflictMode) -> ConflictPolicy {
    match mode {
        ConflictMode::Fail => ConflictPolicy::Fail,
        ConflictMode::Replace => ConflictPolicy::Replace,
        ConflictMode::Uniquify => ConflictPolicy::Uniquify,
    }
}

pub(super) fn export_base_name(path: &str) -> String {
    if path == "-" {
        return "capture".to_owned();
    }
    let stem = Path::new(path)
        .file_stem()
        .and_then(std::ffi::OsStr::to_str)
        .filter(|stem| !stem.is_empty())
        .unwrap_or("capture");
    portable_file_stem(stem)
}

pub(super) fn parse_cdp_url(value: &str) -> Result<Url> {
    parse_cdp_endpoint(value).map_err(|error| match error {
        CdpEndpointError::Parse(error) => PageKnotError::new(
            "pageknot.input.cdp_url",
            ErrorStage::Validation,
            format!("invalid remote browser endpoint: {error}"),
        ),
        CdpEndpointError::Scheme => PageKnotError::new(
            "pageknot.input.cdp_url",
            ErrorStage::Validation,
            "remote browser endpoint must use http, https, ws, or wss",
        ),
    })
}

fn parse_viewport(value: &str) -> Result<Viewport> {
    parse_viewport_text(value).ok_or_else(|| viewport_error(value))
}

fn viewport_error(value: &str) -> PageKnotError {
    PageKnotError::new(
        "pageknot.input.viewport",
        ErrorStage::Validation,
        format!("viewport `{value}` must use WIDTHxHEIGHT with positive integers"),
    )
}

pub(super) fn parse_duration(value: &str) -> Result<std::time::Duration> {
    parse_duration_text(value).ok_or_else(|| duration_error(value))
}

fn duration_error(value: &str) -> PageKnotError {
    PageKnotError::new(
        "pageknot.input.duration",
        ErrorStage::Validation,
        format!("duration `{value}` must use ms, s, m, or h"),
    )
}
