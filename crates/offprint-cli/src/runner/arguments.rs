use std::path::Path;

use offprint::{
    ArtifactFormat, CaptureRequest, CaptureScope, ColorScheme as ModelColorScheme, ConflictPolicy,
    DiagnosticsPolicy, ErrorStage, FormatSpec as ModelFormatSpec, MarkdownOptions, Milliseconds,
    MissingResourcePolicy, NetworkPolicy as ModelNetworkPolicy, OffprintError, PdfOptions,
    ReadinessMode, Result, VerificationMode, Viewport, portable_file_stem,
};

use crate::command::{
    CaptureArguments, ColorScheme, ConflictMode, ContentScope, FormatSpec as CliFormatSpec,
    MissingResources, NetworkPolicy, VerificationModeArg, WaitUntil,
};
use crate::config::value::{parse_duration_text, parse_viewport_text};
use crate::config::{ResolvedConfig, set_readiness_mode};

pub(super) fn require_local_browser(resolved: &ResolvedConfig, operation: &str) -> Result<()> {
    if resolved.uses_remote_browser() {
        return Err(OffprintError::new(
            "offprint.input.browser_selection",
            ErrorStage::Validation,
            format!(
                "{operation} requires a local Chromium-based executable. Set \
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
        request.content.missing_resources = match missing {
            MissingResources::Warn => MissingResourcePolicy::Warn,
            MissingResources::Fail => MissingResourcePolicy::Fail,
        };
    }
    if let Some(scope) = arguments.scope {
        request.content.scope = match scope {
            ContentScope::Page => CaptureScope::Page,
            ContentScope::Selection => CaptureScope::Selection,
        };
        request.content.selector = None;
    }
    if let Some(selector) = &arguments.selector {
        request.content.scope = CaptureScope::Page;
        request.content.selector = Some(selector.clone());
    }
    if arguments.remove_unused_css {
        request.content.optimizations.remove_unused_css = true;
    }
    if arguments.remove_unused_fonts {
        request.content.optimizations.remove_unused_fonts = true;
    }
    if arguments.remove_hidden_elements {
        request.content.optimizations.remove_hidden_elements = true;
    }
    if let Some(level) = arguments.verification {
        request.verification = verification_mode(level);
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

pub(super) const fn verification_mode(mode: VerificationModeArg) -> VerificationMode {
    match mode {
        VerificationModeArg::Static => VerificationMode::Static,
        VerificationModeArg::Offline => VerificationMode::Offline,
    }
}

pub(super) const fn artifact_format(format: CliFormatSpec) -> ArtifactFormat {
    match format {
        CliFormatSpec::Pdf => ArtifactFormat::Pdf,
        CliFormatSpec::Markdown => ArtifactFormat::Markdown,
        CliFormatSpec::Zip => ArtifactFormat::Zip,
        CliFormatSpec::SelfExtractingHtml => ArtifactFormat::SelfExtractingHtml,
        CliFormatSpec::Mhtml => ArtifactFormat::Mhtml,
    }
}

pub(super) const fn export_format(
    format: CliFormatSpec,
    pdf: PdfOptions,
    markdown: MarkdownOptions,
) -> ModelFormatSpec {
    match format {
        CliFormatSpec::Pdf => ModelFormatSpec::Pdf(pdf),
        CliFormatSpec::Markdown => ModelFormatSpec::Markdown(markdown),
        CliFormatSpec::Zip => ModelFormatSpec::Zip,
        CliFormatSpec::SelfExtractingHtml => ModelFormatSpec::SelfExtractingHtml,
        CliFormatSpec::Mhtml => ModelFormatSpec::Mhtml,
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

fn parse_viewport(value: &str) -> Result<Viewport> {
    parse_viewport_text(value).ok_or_else(|| viewport_error(value))
}

fn viewport_error(value: &str) -> OffprintError {
    OffprintError::new(
        "offprint.input.viewport",
        ErrorStage::Validation,
        format!("viewport `{value}` must use WIDTHxHEIGHT with positive integers"),
    )
}

pub(super) fn parse_duration(value: &str) -> Result<std::time::Duration> {
    parse_duration_text(value).ok_or_else(|| duration_error(value))
}

fn duration_error(value: &str) -> OffprintError {
    OffprintError::new(
        "offprint.input.duration",
        ErrorStage::Validation,
        format!("duration `{value}` must use ms, s, m, or h"),
    )
}
