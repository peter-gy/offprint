use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use url::Url;

use crate::{
    ArtifactSpec, ArtifactTarget, BrowserEnvironment, CaptureCredentials, CaptureLimits,
    CapturePolicy, ConflictPolicy, DiagnosticsPolicy, ErrorStage, MAXIMUM_CAPTURE_NODES,
    NetworkPolicy, PageKnotError, PortablePath, ReadinessPolicy, Result, VerificationPolicy,
};

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(tag = "kind", content = "value", rename_all = "kebab-case")]
pub enum BrowserSpec {
    Auto,
    Executable(PortablePath),
    Remote(Url),
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptureRequest {
    pub url: Url,
    pub artifact: ArtifactSpec,
    pub browser: BrowserSpec,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub headed: Option<bool>,
    pub environment: BrowserEnvironment,
    pub readiness: ReadinessPolicy,
    pub capture: CapturePolicy,
    #[serde(default, skip_serializing_if = "CaptureCredentials::is_empty")]
    pub credentials: CaptureCredentials,
    pub network: NetworkPolicy,
    pub limits: CaptureLimits,
    pub verification: VerificationPolicy,
    pub diagnostics: DiagnosticsPolicy,
}

impl CaptureRequest {
    pub fn builder(url: impl AsRef<str>) -> Result<CaptureRequestBuilder> {
        CaptureRequestBuilder::new(url)
    }

    pub fn validate(&self) -> Result<()> {
        validate_url(&self.url, &self.capture)?;
        validate_capture_policy(&self.capture)?;
        validate_browser(&self.browser)?;
        validate_remote_capture(self)?;
        validate_limits(&self.limits)?;
        validate_artifact(&self.artifact, &self.limits)?;
        validate_credentials(&self.credentials)?;

        if self.environment.viewport.width == 0
            || self.environment.viewport.height == 0
            || self.environment.viewport.scale == 0
        {
            return Err(PageKnotError::new(
                "pageknot.input.viewport",
                ErrorStage::Validation,
                "viewport width, height, and scale must be greater than zero",
            ));
        }

        Ok(())
    }
}

#[derive(Clone, Debug)]
pub struct CaptureRequestBuilder {
    request: CaptureRequest,
}

impl CaptureRequestBuilder {
    pub fn new(url: impl AsRef<str>) -> Result<Self> {
        let url = Url::parse(url.as_ref()).map_err(|error| {
            PageKnotError::new(
                "pageknot.input.url",
                ErrorStage::Validation,
                format!("invalid capture URL: {error}"),
            )
        })?;
        let limits = CaptureLimits::default();
        Ok(Self {
            request: CaptureRequest {
                url,
                artifact: ArtifactSpec::html_bytes(limits.artifact_bytes),
                browser: BrowserSpec::Auto,
                headed: None,
                environment: BrowserEnvironment::default(),
                readiness: ReadinessPolicy::default(),
                capture: CapturePolicy::default(),
                credentials: CaptureCredentials::default(),
                network: NetworkPolicy::Standard,
                limits,
                verification: VerificationPolicy::Offline,
                diagnostics: DiagnosticsPolicy::default(),
            },
        })
    }

    #[must_use]
    pub fn output(mut self, path: impl Into<PortablePath>) -> Self {
        self.request.artifact = ArtifactSpec::html_file(path.into());
        self
    }

    #[must_use]
    pub fn conflict(mut self, conflict: ConflictPolicy) -> Self {
        let ArtifactSpec::Html(options) = &mut self.request.artifact;
        options.conflict = conflict;
        self
    }

    #[must_use]
    pub fn browser(mut self, browser: BrowserSpec) -> Self {
        self.request.browser = browser;
        self
    }

    #[must_use]
    pub fn environment(mut self, environment: BrowserEnvironment) -> Self {
        self.request.environment = environment;
        self
    }

    #[must_use]
    pub fn readiness(mut self, readiness: ReadinessPolicy) -> Self {
        self.request.readiness = readiness;
        self
    }

    #[must_use]
    pub fn capture_policy(mut self, capture: CapturePolicy) -> Self {
        self.request.capture = capture;
        self
    }

    #[must_use]
    pub fn network(mut self, network: NetworkPolicy) -> Self {
        self.request.network = network;
        self
    }

    #[must_use]
    pub fn limits(mut self, limits: CaptureLimits) -> Self {
        self.request.limits = limits;
        self
    }

    #[must_use]
    pub fn verification(mut self, verification: VerificationPolicy) -> Self {
        self.request.verification = verification;
        self
    }

    #[must_use]
    pub fn diagnostics(mut self, diagnostics: DiagnosticsPolicy) -> Self {
        self.request.diagnostics = diagnostics;
        self
    }

    pub fn build(self) -> Result<CaptureRequest> {
        self.request.validate()?;
        Ok(self.request)
    }
}

fn validate_capture_policy(capture: &CapturePolicy) -> Result<()> {
    if capture.selector.is_some() && capture.scope == crate::CaptureScope::Selection {
        return Err(PageKnotError::new(
            "pageknot.input.selector_scope",
            ErrorStage::Validation,
            "choose either a DOM selector or the active selection capture scope",
        ));
    }
    Ok(())
}

fn validate_url(url: &Url, capture: &CapturePolicy) -> Result<()> {
    match url.scheme() {
        "http" | "https" => Ok(()),
        "file" if !capture.allowed_file_roots.is_empty() => Ok(()),
        "file" => Err(PageKnotError::new(
            "pageknot.input.file_root",
            ErrorStage::Validation,
            "file capture requires at least one allowed root",
        )),
        scheme => Err(PageKnotError::new(
            "pageknot.input.url_scheme",
            ErrorStage::Validation,
            format!("URL scheme `{scheme}` cannot be captured"),
        )),
    }
}

fn validate_browser(browser: &BrowserSpec) -> Result<()> {
    if let BrowserSpec::Remote(endpoint) = browser
        && !matches!(endpoint.scheme(), "http" | "https" | "ws" | "wss")
    {
        return Err(PageKnotError::new(
            "pageknot.input.cdp_url",
            ErrorStage::Validation,
            "remote browser endpoint must use http, https, ws, or wss",
        ));
    }
    Ok(())
}

fn validate_remote_capture(request: &CaptureRequest) -> Result<()> {
    if !matches!(request.browser, BrowserSpec::Remote(_)) {
        return Ok(());
    }
    if !matches!(request.network, NetworkPolicy::Unrestricted) {
        return Err(PageKnotError::new(
            "pageknot.input.remote_network_policy",
            ErrorStage::Validation,
            "remote browser capture requires the unrestricted network policy",
        ));
    }
    if request.verification != VerificationPolicy::Static {
        return Err(PageKnotError::new(
            "pageknot.input.remote_verification_policy",
            ErrorStage::Validation,
            "remote browser capture requires static verification",
        ));
    }
    Ok(())
}

fn validate_limits(limits: &CaptureLimits) -> Result<()> {
    let has_zero = limits.duration.get() == 0
        || limits.frames == 0
        || limits.nodes == 0
        || limits.resources == 0
        || limits.resource_bytes == 0
        || limits.total_resource_bytes == 0
        || limits.collector_chunk_bytes == 0
        || limits.concurrent_resources == 0
        || limits.artifact_bytes == 0
        || limits.css_import_depth == 0
        || limits.frame_depth == 0;
    if has_zero {
        return Err(PageKnotError::new(
            "pageknot.input.limit",
            ErrorStage::Validation,
            "capture limits must be greater than zero",
        ));
    }
    if limits.nodes > MAXIMUM_CAPTURE_NODES {
        return Err(PageKnotError::new(
            "pageknot.input.limit",
            ErrorStage::Validation,
            "capture DOM node limit exceeds the supported maximum",
        )
        .with_detail("attempted", limits.nodes)
        .with_detail("limit", MAXIMUM_CAPTURE_NODES));
    }
    if limits.total_resource_bytes < limits.resource_bytes {
        return Err(PageKnotError::new(
            "pageknot.input.limit",
            ErrorStage::Validation,
            "total resource bytes must be at least the individual resource limit",
        ));
    }
    Ok(())
}

fn validate_artifact(artifact: &ArtifactSpec, limits: &CaptureLimits) -> Result<()> {
    let ArtifactSpec::Html(options) = artifact;
    match &options.target {
        ArtifactTarget::File(path) if path.as_str().is_empty() => Err(PageKnotError::new(
            "pageknot.input.output",
            ErrorStage::Validation,
            "output path must contain a file name",
        )),
        ArtifactTarget::Bytes { max_bytes } if *max_bytes == 0 => Err(PageKnotError::new(
            "pageknot.input.limit",
            ErrorStage::Validation,
            "in-memory artifact limit must be greater than zero",
        )),
        ArtifactTarget::Bytes { max_bytes } if *max_bytes > limits.artifact_bytes => {
            Err(PageKnotError::new(
                "pageknot.input.limit",
                ErrorStage::Validation,
                "in-memory artifact target exceeds the capture artifact limit",
            ))
        }
        ArtifactTarget::File(_) | ArtifactTarget::Bytes { .. } => Ok(()),
    }
}

fn validate_credentials(credentials: &CaptureCredentials) -> Result<()> {
    if credentials.headers.len() > 256 || credentials.cookies.len() > 4096 {
        return Err(PageKnotError::new(
            "pageknot.input.credentials",
            ErrorStage::Validation,
            "credential entry count exceeds the supported limit",
        ));
    }
    let mut header_names = std::collections::BTreeSet::new();
    for header in &credentials.headers {
        let normalized = header.name.to_ascii_lowercase();
        if !is_http_token(&header.name)
            || matches!(
                normalized.as_str(),
                "connection"
                    | "content-length"
                    | "cookie"
                    | "host"
                    | "proxy-authorization"
                    | "set-cookie"
                    | "transfer-encoding"
            )
            || normalized.starts_with("sec-")
            || !header_names.insert(normalized)
            || contains_header_control(header.value.expose_secret())
        {
            return Err(PageKnotError::new(
                "pageknot.input.credentials",
                ErrorStage::Validation,
                "request headers contain an invalid, duplicate, or browser-controlled entry",
            ));
        }
    }
    for cookie in &credentials.cookies {
        if cookie.name.is_empty()
            || cookie.name.len() > 4096
            || cookie
                .name
                .bytes()
                .any(|byte| byte <= 0x20 || matches!(byte, b';' | b','))
            || cookie.value.expose_secret().len() > 4096
            || contains_cookie_control(cookie.value.expose_secret())
            || cookie.url.is_none() && cookie.domain.is_none()
            || cookie.domain.as_ref().is_some_and(String::is_empty)
            || cookie
                .url
                .as_ref()
                .is_some_and(|url| !matches!(url.scheme(), "http" | "https"))
        {
            return Err(PageKnotError::new(
                "pageknot.input.credentials",
                ErrorStage::Validation,
                "cookie input contains an invalid name, value, or scope",
            ));
        }
    }
    Ok(())
}

fn is_http_token(value: &str) -> bool {
    !value.is_empty()
        && value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric()
                || matches!(
                    byte,
                    b'!' | b'#'
                        | b'$'
                        | b'%'
                        | b'&'
                        | b'\''
                        | b'*'
                        | b'+'
                        | b'-'
                        | b'.'
                        | b'^'
                        | b'_'
                        | b'`'
                        | b'|'
                        | b'~'
                )
        })
}

fn contains_header_control(value: &str) -> bool {
    value
        .bytes()
        .any(|byte| byte == 0 || byte == b'\r' || byte == b'\n')
}

fn contains_cookie_control(value: &str) -> bool {
    value
        .bytes()
        .any(|byte| byte < 0x20 || byte == 0x7f || matches!(byte, b';' | b','))
}

#[cfg(test)]
mod tests {
    use super::CaptureRequest;
    use crate::{
        ArtifactSpec, BrowserSpec, CaptureLimits, CaptureScope, ConflictPolicy,
        MAXIMUM_CAPTURE_NODES, NetworkPolicy, RequestHeader, SecretString, VerificationPolicy,
    };
    use url::Url;

    #[test]
    fn request_builder_applies_the_versioned_desktop_defaults() {
        let request =
            CaptureRequest::builder("https://example.com").and_then(|builder| builder.build());
        let request = request.as_ref().ok();

        assert_eq!(
            request.map(|value| value.environment.viewport.width),
            Some(1440)
        );
        assert_eq!(
            request.map(|value| value.environment.viewport.height),
            Some(900)
        );
        assert_eq!(
            request.map(|value| value.limits.duration.get()),
            Some(120_000)
        );
    }

    #[test]
    fn file_output_defaults_to_atomic_replacement() {
        let request = CaptureRequest::builder("https://example.com")
            .map(|builder| builder.output("capture.html"))
            .and_then(|builder| builder.build());
        let conflict = request.as_ref().map(|request| {
            let ArtifactSpec::Html(artifact) = &request.artifact;
            artifact.conflict
        });

        assert_eq!(conflict, Ok(ConflictPolicy::Replace));
    }

    #[test]
    fn request_validation_rejects_file_urls_without_an_allowed_root() {
        let result =
            CaptureRequest::builder("file:///tmp/page.html").and_then(|builder| builder.build());

        assert_eq!(
            result.as_ref().map_err(|error| error.code.as_str()),
            Err("pageknot.input.file_root")
        );
    }

    #[test]
    fn request_validation_rejects_browser_controlled_secret_headers() {
        let request = CaptureRequest::builder("https://example.com")
            .and_then(|builder| builder.build())
            .map(|mut request| {
                request.credentials.headers.push(RequestHeader {
                    name: "Host".to_owned(),
                    value: SecretString::new("internal.example"),
                });
                request
            });
        let validated = request.and_then(|request| request.validate());

        assert_eq!(
            validated.as_ref().map_err(|error| error.code.as_str()),
            Err("pageknot.input.credentials")
        );
    }

    #[test]
    fn request_validation_rejects_selector_with_active_selection_scope() {
        let request = CaptureRequest::builder("https://example.com")
            .and_then(|builder| builder.build())
            .map(|mut request| {
                request.capture.selector = Some("main".to_owned());
                request.capture.scope = CaptureScope::Selection;
                request
            });
        let validated = request.and_then(|request| request.validate());

        assert_eq!(
            validated.as_ref().map_err(|error| error.code.as_str()),
            Err("pageknot.input.selector_scope")
        );
    }

    #[test]
    fn request_validation_accepts_the_supported_node_maximum() {
        let limits = CaptureLimits {
            nodes: MAXIMUM_CAPTURE_NODES,
            ..CaptureLimits::default()
        };

        let result = CaptureRequest::builder("https://example.com")
            .map(|builder| builder.limits(limits))
            .and_then(|builder| builder.build());

        assert_eq!(
            result.as_ref().map(|request| request.limits.nodes),
            Ok(MAXIMUM_CAPTURE_NODES)
        );
    }

    #[test]
    fn request_validation_rejects_a_node_limit_above_the_supported_maximum() {
        let attempted = MAXIMUM_CAPTURE_NODES + 1;
        let limits = CaptureLimits {
            nodes: attempted,
            ..CaptureLimits::default()
        };

        let result = CaptureRequest::builder("https://example.com")
            .map(|builder| builder.limits(limits))
            .and_then(|builder| builder.build());
        let error = result.as_ref().err();

        assert_eq!(
            error.map(|error| error.code.as_str()),
            Some("pageknot.input.limit")
        );
        assert_eq!(
            error.and_then(|error| error.details.get("attempted")),
            Some(&serde_json::json!(attempted))
        );
        assert_eq!(
            error.and_then(|error| error.details.get("limit")),
            Some(&serde_json::json!(MAXIMUM_CAPTURE_NODES))
        );
    }

    #[test]
    fn remote_capture_requires_unrestricted_networking_before_job_registration()
    -> std::result::Result<(), url::ParseError> {
        let endpoint = Url::parse("http://127.0.0.1:9222")?;
        let result = CaptureRequest::builder("https://example.com")
            .map(|builder| {
                builder
                    .browser(BrowserSpec::Remote(endpoint))
                    .verification(VerificationPolicy::Static)
            })
            .and_then(|builder| builder.build());

        assert_eq!(
            result.as_ref().map_err(|error| error.code.as_str()),
            Err("pageknot.input.remote_network_policy")
        );
        Ok(())
    }

    #[test]
    fn remote_capture_requires_static_verification_before_job_registration()
    -> std::result::Result<(), url::ParseError> {
        let endpoint = Url::parse("http://127.0.0.1:9222")?;
        let result = CaptureRequest::builder("https://example.com")
            .map(|builder| {
                builder
                    .browser(BrowserSpec::Remote(endpoint))
                    .network(NetworkPolicy::Unrestricted)
            })
            .and_then(|builder| builder.build());

        assert_eq!(
            result.as_ref().map_err(|error| error.code.as_str()),
            Err("pageknot.input.remote_verification_policy")
        );
        Ok(())
    }

    #[test]
    fn remote_capture_accepts_the_supported_policy_pair() -> std::result::Result<(), url::ParseError>
    {
        let endpoint = Url::parse("http://127.0.0.1:9222")?;
        let result = CaptureRequest::builder("https://example.com")
            .map(|builder| {
                builder
                    .browser(BrowserSpec::Remote(endpoint))
                    .network(NetworkPolicy::Unrestricted)
                    .verification(VerificationPolicy::Static)
            })
            .and_then(|builder| builder.build());

        assert!(result.is_ok());
        Ok(())
    }
}
