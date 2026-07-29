use clap::{Args, Parser, Subcommand, ValueEnum};

#[derive(Clone, Debug, Parser)]
#[command(
    name = "pageknot",
    version,
    about = "Capture rendered web pages as verified self-contained files"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Clone, Debug, Subcommand)]
pub enum Command {
    /// Capture a rendered page and verify the saved artifact.
    #[command(
        after_help = "Examples:\n  pageknot capture https://example.com\n  pageknot capture https://example.com --wait-until network-idle --delay 1s\n  pageknot capture https://example.com --selector 'main article' -o article.html\n  pageknot capture https://example.com --format pdf -o example.pdf\n  pageknot capture https://example.com -o example.html --verify offline\n  pageknot capture https://example.com --json"
    )]
    Capture(Box<CaptureArguments>),
    /// Derive independently verified formats from one PageKnot HTML artifact.
    Export(ExportArguments),
    /// Run independent capture requests from a JSON batch manifest.
    Batch(BatchArguments),
    /// Capture a bounded breadth-first set of linked pages.
    Crawl(CrawlArguments),
    /// Verify an existing PageKnot artifact.
    Verify(VerifyArguments),
    /// Read and validate an artifact manifest.
    Inspect(InspectArguments),
    /// Check configuration, browser discovery, and output capabilities.
    Doctor(DoctorArguments),
    /// Install, list, or remove managed Chromium builds.
    Browser(BrowserArguments),
    /// Print a shell completion script to stdout.
    Completion(CompletionArguments),
}

#[derive(Clone, Debug, Args)]
pub struct BatchArguments {
    /// JSON `BatchRequest` path. Use `-` for stdin.
    #[arg(value_name = "MANIFEST")]
    pub manifest: String,

    /// Read service configuration from this TOML file.
    #[arg(long, value_name = "PATH")]
    pub config: Option<String>,

    /// Use this Chrome or Chromium executable.
    #[arg(long, value_name = "PATH", conflicts_with = "cdp_url")]
    pub browser_path: Option<String>,

    /// Connect to an HTTP or WebSocket CDP endpoint when every request uses
    /// static verification and unrestricted network policy.
    #[arg(long, value_name = "URL", conflicts_with = "browser_path")]
    pub cdp_url: Option<String>,

    #[command(flatten)]
    pub output_options: OutputOptions,
}

#[derive(Clone, Debug, Args)]
pub struct CrawlArguments {
    /// HTTP or HTTPS seed URL.
    #[arg(value_name = "URL")]
    pub url: String,

    /// Directory that receives one verified HTML file per page.
    #[arg(short, long, value_name = "DIR")]
    pub output: String,

    /// Apply a named capture profile to every page.
    #[arg(long, value_name = "NAME")]
    pub profile: Option<String>,

    /// Read service configuration from this TOML file.
    #[arg(long, value_name = "PATH")]
    pub config: Option<String>,

    /// Capture at most this many pages.
    #[arg(long, default_value_t = 100)]
    pub max_pages: u32,

    /// Follow links through this many breadth-first levels.
    #[arg(long, default_value_t = 3)]
    pub max_depth: u16,

    /// Run at most this many captures at once.
    #[arg(long, default_value_t = 4)]
    pub concurrency: u16,

    /// Persist atomic resume state at this path.
    #[arg(long, value_name = "PATH")]
    pub resume: Option<String>,

    /// Schedule pages that previously reached a failed terminal state.
    #[arg(long, requires = "resume")]
    pub retry_failed: bool,

    /// Follow HTTP and HTTPS links outside the seed origin.
    #[arg(long)]
    pub allow_cross_origin: bool,

    /// Use this Chrome or Chromium executable.
    #[arg(long, value_name = "PATH")]
    pub browser_path: Option<String>,

    /// Display browser windows during the crawl.
    #[arg(long)]
    pub headed: bool,

    #[command(flatten)]
    pub output_options: OutputOptions,
}

#[derive(Clone, Debug, Args)]
pub struct CaptureArguments {
    /// Absolute HTTP, HTTPS, or permitted file URL to capture.
    #[arg(value_name = "URL")]
    pub url: String,

    /// Write the artifact to this path. Use `-` for stdout.
    #[arg(short, long, value_name = "PATH")]
    pub output: Option<String>,

    /// Select the committed capture representation.
    #[arg(long, value_enum, default_value_t = CaptureFormat::Html)]
    pub format: CaptureFormat,

    /// Print PDF pages in landscape orientation.
    #[arg(long)]
    pub landscape: bool,

    /// Honor the captured document's CSS page size when printing PDF.
    #[arg(long)]
    pub prefer_css_page_size: bool,

    /// Apply a named configuration profile.
    #[arg(long, value_name = "NAME")]
    pub profile: Option<String>,

    /// Read configuration from this TOML file.
    #[arg(long, value_name = "PATH")]
    pub config: Option<String>,

    /// Use this Chrome or Chromium executable.
    #[arg(long, value_name = "PATH", conflicts_with = "cdp_url")]
    pub browser_path: Option<String>,

    /// Connect to an HTTP or WebSocket CDP endpoint with `--verify static` and
    /// `--network-policy unrestricted`.
    #[arg(long, value_name = "URL", conflicts_with = "browser_path")]
    pub cdp_url: Option<String>,

    /// Display the browser window during capture.
    #[arg(long)]
    pub headed: bool,

    /// Set the CSS viewport as WIDTHxHEIGHT.
    #[arg(long, value_name = "WIDTHxHEIGHT")]
    pub viewport: Option<String>,

    /// Set the browser locale, such as en-US.
    #[arg(long, value_name = "LOCALE")]
    pub locale: Option<String>,

    /// Set an IANA timezone, such as Europe/Vienna.
    #[arg(long, value_name = "ZONE")]
    pub timezone: Option<String>,

    /// Emulate the page color preference.
    #[arg(long, value_enum)]
    pub color_scheme: Option<ColorScheme>,

    /// Set the total capture deadline, such as 30s or 2m.
    #[arg(long, value_name = "DURATION")]
    pub timeout: Option<String>,

    /// Select the browser readiness condition.
    #[arg(long, value_enum)]
    pub wait_until: Option<WaitUntil>,

    /// Wait this long after the selected readiness condition.
    #[arg(long, value_name = "DURATION")]
    pub delay: Option<String>,

    /// Select the outcome for unresolved resources.
    #[arg(long, value_enum)]
    pub missing_resources: Option<MissingResources>,

    /// Capture the complete page or its active top-level selection.
    #[arg(long, value_enum)]
    pub scope: Option<ContentScope>,

    /// Capture the first top-level document element matching this CSS selector.
    #[arg(long, value_name = "CSS", conflicts_with = "scope")]
    pub selector: Option<String>,

    /// Remove CSS rules that cannot match the captured document state.
    #[arg(long)]
    pub remove_unused_css: bool,

    /// Remove font faces unused by the captured document state.
    #[arg(long)]
    pub remove_unused_fonts: bool,

    /// Remove elements whose computed display is none.
    #[arg(long)]
    pub remove_hidden_elements: bool,

    /// Select the required artifact verification level.
    #[arg(long, value_enum)]
    pub verify: Option<VerificationLevel>,

    /// Read request headers from a protected JSON file or stdin.
    #[arg(long, value_name = "PATH|-")]
    pub headers: Option<String>,

    /// Read browser cookies from a protected JSON file or stdin.
    #[arg(long, value_name = "PATH|-")]
    pub cookies: Option<String>,

    /// Select address and redirect restrictions.
    #[arg(long, value_enum)]
    pub network_policy: Option<NetworkPolicy>,

    #[command(flatten)]
    pub output_options: OutputOptions,

    /// Write sanitized failure diagnostics to this directory.
    #[arg(long, value_name = "DIR")]
    pub diagnostics: Option<String>,
}

#[derive(Clone, Debug, Args)]
pub struct ExportArguments {
    /// PageKnot HTML artifact path. Use `-` for stdin.
    #[arg(value_name = "ARTIFACT")]
    pub artifact: ArtifactArgument,

    /// Directory that receives the exported formats.
    #[arg(short, long, value_name = "DIR")]
    pub output: String,

    /// Portable output stem. Defaults to the source file stem.
    #[arg(long, value_name = "NAME")]
    pub base_name: Option<String>,

    /// Format to derive. Repeat the flag or pass a comma-separated list.
    #[arg(
        long,
        value_enum,
        value_delimiter = ',',
        required = true,
        num_args = 1..
    )]
    pub variant: Vec<ArtifactVariant>,

    /// Print PDF pages in landscape orientation.
    #[arg(long)]
    pub landscape: bool,

    /// Honor the captured document's CSS page size when printing PDF.
    #[arg(long)]
    pub prefer_css_page_size: bool,

    /// Omit provenance front matter from Markdown output.
    #[arg(long)]
    pub no_front_matter: bool,

    /// Select behavior when an export destination already exists.
    #[arg(long, value_enum, default_value_t = ConflictMode::Replace)]
    pub conflict: ConflictMode,

    /// Read service configuration from this TOML file.
    #[arg(long, value_name = "PATH")]
    pub config: Option<String>,

    /// Use this Chrome or Chromium executable.
    #[arg(long, value_name = "PATH")]
    pub browser_path: Option<String>,

    #[command(flatten)]
    pub output_options: OutputOptions,
}

#[derive(Clone, Debug, Args)]
pub struct VerifyArguments {
    /// Artifact path. HTML input may use `-` for stdin.
    #[arg(value_name = "ARTIFACT")]
    pub artifact: ArtifactArgument,

    /// Select an alternate exported format. Omit for PageKnot HTML.
    #[arg(long, value_enum)]
    pub format: Option<ArtifactVariant>,

    /// Select static checks or a network-denied browser reopen.
    #[arg(long, value_enum)]
    pub level: Option<VerificationLevel>,

    /// Read configuration from this TOML file.
    #[arg(long, value_name = "PATH")]
    pub config: Option<String>,

    /// Use this Chrome or Chromium executable for offline verification.
    #[arg(long, value_name = "PATH")]
    pub browser_path: Option<String>,

    /// Set the verification deadline, such as 30s.
    #[arg(long, value_name = "DURATION")]
    pub timeout: Option<String>,

    #[command(flatten)]
    pub output_options: OutputOptions,

    /// Write sanitized failure diagnostics to this directory.
    #[arg(long, value_name = "DIR")]
    pub diagnostics: Option<String>,
}

#[derive(Clone, Debug, Args)]
pub struct InspectArguments {
    /// PageKnot HTML artifact path. Use `-` for stdin.
    #[arg(value_name = "ARTIFACT")]
    pub artifact: ArtifactArgument,

    #[command(flatten)]
    pub output_options: OutputOptions,
}

#[derive(Clone, Debug, Args)]
pub struct DoctorArguments {
    /// Read configuration from this TOML file.
    #[arg(long, value_name = "PATH")]
    pub config: Option<String>,

    /// Check this Chrome or Chromium executable.
    #[arg(long, value_name = "PATH", conflicts_with = "cdp_url")]
    pub browser_path: Option<String>,

    /// Check this existing HTTP or WebSocket CDP endpoint.
    #[arg(long, value_name = "URL", conflicts_with = "browser_path")]
    pub cdp_url: Option<String>,

    #[command(flatten)]
    pub output_options: OutputOptions,
}

#[derive(Clone, Debug, Args)]
pub struct BrowserArguments {
    #[command(subcommand)]
    pub command: BrowserCommand,
}

#[derive(Clone, Debug, Subcommand)]
pub enum BrowserCommand {
    /// Install a trusted managed Chromium revision.
    Install(BrowserInstallArguments),
    /// List managed and discovered browser candidates.
    List(BrowserListArguments),
    /// Remove a managed Chromium revision.
    Remove(BrowserRemoveArguments),
}

#[derive(Clone, Debug, Args)]
pub struct BrowserInstallArguments {
    /// Install this trusted revision.
    #[arg(long, value_name = "REVISION")]
    pub revision: Option<String>,

    /// Use this managed browser cache.
    #[arg(long, value_name = "DIR")]
    pub cache_dir: Option<String>,

    #[command(flatten)]
    pub output_options: OutputOptions,
}

#[derive(Clone, Debug, Args)]
pub struct BrowserListArguments {
    /// Use this managed browser cache.
    #[arg(long, value_name = "DIR")]
    pub cache_dir: Option<String>,

    #[command(flatten)]
    pub output_options: OutputOptions,
}

#[derive(Clone, Debug, Args)]
pub struct BrowserRemoveArguments {
    /// Managed revision identifier to remove.
    #[arg(value_name = "REVISION")]
    pub revision: String,

    /// Use this managed browser cache.
    #[arg(long, value_name = "DIR")]
    pub cache_dir: Option<String>,

    /// Remove the selected idle revision after resolving a replacement.
    #[arg(long)]
    pub force: bool,

    #[command(flatten)]
    pub output_options: OutputOptions,
}

#[derive(Clone, Debug, Args)]
pub struct CompletionArguments {
    /// Shell whose completion script should be generated.
    #[arg(value_enum)]
    pub shell: CompletionShell,
}

#[derive(Clone, Debug, Args)]
pub struct OutputOptions {
    /// Write one versioned JSON object to stdout.
    #[arg(long)]
    pub json: bool,

    /// Suppress non-error human diagnostics.
    #[arg(long)]
    pub quiet: bool,

    /// Control color in human diagnostics.
    #[arg(long, value_enum, default_value_t = ColorOutput::Auto)]
    pub color: ColorOutput,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub enum VerificationLevel {
    Static,
    Offline,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, ValueEnum)]
pub enum CaptureFormat {
    #[default]
    Html,
    Pdf,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub enum ArtifactVariant {
    Pdf,
    Markdown,
    Zip,
    SelfExtracting,
    Mhtml,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub enum ConflictMode {
    Fail,
    Replace,
    Uniquify,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub enum CompletionShell {
    Bash,
    Elvish,
    Fish,
    Powershell,
    Zsh,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub enum ColorOutput {
    Auto,
    Always,
    Never,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub enum ColorScheme {
    Light,
    Dark,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub enum WaitUntil {
    RenderIdle,
    NetworkIdle,
    Load,
    DomContentLoaded,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub enum MissingResources {
    Warn,
    Fail,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub enum ContentScope {
    Page,
    Selection,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub enum NetworkPolicy {
    Standard,
    Server,
    Unrestricted,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ArtifactArgument(pub String);

impl std::str::FromStr for ArtifactArgument {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        if value.is_empty() {
            Err("artifact path must not be empty".to_owned())
        } else {
            Ok(Self(value.to_owned()))
        }
    }
}

#[cfg(test)]
mod tests {
    use clap::Parser as _;

    use super::{ArtifactVariant, CaptureFormat, Cli, Command, ConflictMode};

    #[test]
    fn capture_rejects_two_browser_ownership_modes() {
        let result = Cli::try_parse_from([
            "pageknot",
            "capture",
            "https://example.com",
            "--browser-path",
            "/browser",
            "--cdp-url",
            "ws://localhost:9222",
        ]);

        assert!(result.is_err());
    }

    #[test]
    fn command_tree_uses_capture_as_the_canonical_verb() {
        let parsed = Cli::try_parse_from([
            "pageknot",
            "capture",
            "https://example.com",
            "--verify",
            "offline",
        ]);

        assert!(matches!(
            parsed.as_ref().map(|cli| &cli.command),
            Ok(Command::Capture(_))
        ));
    }

    #[test]
    fn capture_parses_selector_selection_and_optimizer_contracts() {
        let parsed = Cli::try_parse_from([
            "pageknot",
            "capture",
            "https://example.com",
            "--selector",
            "main article",
            "--remove-unused-css",
            "--remove-unused-fonts",
            "--remove-hidden-elements",
        ]);
        let arguments = parsed.as_ref().ok().and_then(|cli| match &cli.command {
            Command::Capture(arguments) => Some(arguments),
            _ => None,
        });

        assert_eq!(
            arguments.and_then(|arguments| arguments.selector.as_deref()),
            Some("main article")
        );
        assert!(arguments.is_some_and(|arguments| arguments.remove_unused_css));
        assert!(arguments.is_some_and(|arguments| arguments.remove_unused_fonts));
        assert!(arguments.is_some_and(|arguments| arguments.remove_hidden_elements));
    }

    #[test]
    fn capture_accepts_pdf_print_options() {
        let parsed = Cli::try_parse_from([
            "pageknot",
            "capture",
            "https://example.com",
            "--format",
            "pdf",
            "--output",
            "capture.pdf",
            "--landscape",
            "--prefer-css-page-size",
        ]);
        let arguments = parsed.as_ref().ok().and_then(|cli| match &cli.command {
            Command::Capture(arguments) => Some(arguments),
            _ => None,
        });

        assert_eq!(
            arguments.map(|arguments| arguments.format),
            Some(CaptureFormat::Pdf)
        );
        assert!(arguments.is_some_and(|arguments| arguments.landscape));
        assert!(arguments.is_some_and(|arguments| arguments.prefer_css_page_size));
    }

    #[test]
    fn capture_rejects_selector_with_active_selection_scope() {
        let parsed = Cli::try_parse_from([
            "pageknot",
            "capture",
            "https://example.com",
            "--selector",
            "main",
            "--scope",
            "selection",
        ]);

        assert!(parsed.is_err());
    }

    #[test]
    fn crawl_defaults_to_a_bounded_same_origin_plan() {
        let parsed = Cli::try_parse_from([
            "pageknot",
            "crawl",
            "https://example.com",
            "--output",
            "archive",
            "--resume",
            "crawl.json",
        ]);
        let arguments = parsed.as_ref().ok().and_then(|cli| match &cli.command {
            Command::Crawl(arguments) => Some(arguments),
            _ => None,
        });

        assert_eq!(arguments.map(|arguments| arguments.max_pages), Some(100));
        assert_eq!(arguments.map(|arguments| arguments.max_depth), Some(3));
        assert_eq!(arguments.map(|arguments| arguments.concurrency), Some(4));
        assert!(arguments.is_some_and(|arguments| !arguments.allow_cross_origin));
    }

    #[test]
    fn export_accepts_repeated_and_comma_separated_variants() {
        let parsed = Cli::try_parse_from([
            "pageknot",
            "export",
            "capture.html",
            "--output",
            "formats",
            "--variant",
            "pdf,markdown",
            "--variant",
            "zip",
        ]);
        let arguments = parsed.as_ref().ok().and_then(|cli| match &cli.command {
            Command::Export(arguments) => Some(arguments),
            _ => None,
        });

        assert_eq!(
            arguments.map(|arguments| arguments.variant.as_slice()),
            Some(
                [
                    ArtifactVariant::Pdf,
                    ArtifactVariant::Markdown,
                    ArtifactVariant::Zip,
                ]
                .as_slice()
            )
        );
        assert_eq!(
            arguments.map(|arguments| arguments.conflict),
            Some(ConflictMode::Replace)
        );
    }

    #[test]
    fn offline_commands_reject_remote_cdp_selection() {
        for arguments in [
            vec![
                "pageknot",
                "export",
                "capture.html",
                "--output",
                "formats",
                "--variant",
                "pdf",
                "--cdp-url",
                "ws://localhost:9222",
            ],
            vec![
                "pageknot",
                "verify",
                "capture.html",
                "--level",
                "offline",
                "--cdp-url",
                "ws://localhost:9222",
            ],
            vec![
                "pageknot",
                "crawl",
                "https://example.com",
                "--output",
                "archive",
                "--cdp-url",
                "ws://localhost:9222",
            ],
        ] {
            let result = Cli::try_parse_from(arguments);

            assert!(
                result.is_err(),
                "offline command accepted remote CDP selection"
            );
            if let Err(error) = result {
                assert_eq!(error.kind(), clap::error::ErrorKind::UnknownArgument);
                assert!(error.to_string().contains("--cdp-url"));
            }
        }
    }

    #[test]
    fn offline_command_help_exposes_local_browser_selection() {
        for command in ["export", "verify", "crawl"] {
            let result = Cli::try_parse_from(["pageknot", command, "--help"]);

            assert!(result.is_err(), "help request parsed as a command");
            if let Err(error) = result {
                let help = error.to_string();
                assert_eq!(error.kind(), clap::error::ErrorKind::DisplayHelp);
                assert!(help.contains("--browser-path"), "{command}:\n{help}");
                assert!(!help.contains("--cdp-url"), "{command}:\n{help}");
            }
        }
    }

    #[test]
    fn offline_commands_accept_local_browser_selection() {
        for arguments in [
            vec![
                "pageknot",
                "export",
                "capture.html",
                "--output",
                "formats",
                "--variant",
                "pdf",
                "--browser-path",
                "/browser",
            ],
            vec![
                "pageknot",
                "verify",
                "capture.html",
                "--level",
                "offline",
                "--browser-path",
                "/browser",
            ],
            vec![
                "pageknot",
                "crawl",
                "https://example.com",
                "--output",
                "archive",
                "--browser-path",
                "/browser",
            ],
        ] {
            assert!(
                Cli::try_parse_from(arguments).is_ok(),
                "offline command rejected local browser selection"
            );
        }
    }

    #[test]
    fn supported_commands_keep_remote_cdp_selection() {
        for arguments in [
            vec![
                "pageknot",
                "capture",
                "https://example.com",
                "--cdp-url",
                "ws://localhost:9222",
                "--verify",
                "static",
                "--network-policy",
                "unrestricted",
            ],
            vec![
                "pageknot",
                "batch",
                "batch.json",
                "--cdp-url",
                "ws://localhost:9222",
            ],
            vec!["pageknot", "doctor", "--cdp-url", "ws://localhost:9222"],
        ] {
            assert!(
                Cli::try_parse_from(arguments).is_ok(),
                "supported command rejected remote CDP selection"
            );
        }
    }

    #[test]
    fn remote_cdp_help_states_the_capture_policy() {
        let capture = Cli::try_parse_from(["pageknot", "capture", "--help"]);
        let batch = Cli::try_parse_from(["pageknot", "batch", "--help"]);

        assert!(capture.is_err());
        assert!(batch.is_err());
        if let Err(error) = capture {
            let help = error
                .to_string()
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ");
            assert!(help.contains("--verify static"));
            assert!(help.contains("--network-policy unrestricted"));
        }
        if let Err(error) = batch {
            let help = error
                .to_string()
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ");
            assert!(help.contains("every request uses static verification"));
            assert!(help.contains("unrestricted network policy"));
        }
    }
}
