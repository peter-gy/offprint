use std::collections::BTreeMap;

use base64::Engine as _;
use data_url::DataUrl;
use pageknot_browser::{BodyStream, LoadedResource, NetworkGuard, PageSession};
use pageknot_capture::{CaptureBudget, ContentStore, StoredContent};
use pageknot_document::{Document, RenderingRole};
use pageknot_model::{
    CaptureRequest, ContentDigest, ErrorStage, MissingResourcePolicy, PageKnotError, RedactedUrl,
    RedactionPolicy, ResourceId, ResourceOutcome, ResourceProvenance, ResourceRecord,
    ResourceRetrievalSource, Result,
};
use pageknot_protocol::{PageObservation, VisualFallbackKind};
use url::Url;

#[derive(Debug)]
pub(super) struct StoredResourceBytes {
    pub(super) bytes: Vec<u8>,
    pub(super) content: StoredContent,
}

#[derive(Debug)]
pub(super) struct LoadedResourceData {
    pub(super) bytes: Vec<u8>,
    pub(super) content: StoredContent,
    pub(super) media_type: String,
    pub(super) provenance: ResourceProvenance,
}

#[derive(Debug)]
pub(super) struct ResourceMaterializer {
    content_store: ContentStore,
}

impl ResourceMaterializer {
    pub(super) fn new() -> Result<Self> {
        Ok(Self {
            content_store: ContentStore::new()?,
        })
    }

    pub(super) async fn store_bytes(
        &mut self,
        bytes: Vec<u8>,
        maximum_resource_bytes: u64,
        maximum_total_bytes: u64,
    ) -> Result<StoredResourceBytes> {
        let inserted = self
            .content_store
            .insert_bytes(bytes, maximum_resource_bytes, maximum_total_bytes)
            .await?;
        let bytes = inserted.content.read(maximum_resource_bytes).await?;
        Ok(StoredResourceBytes {
            bytes,
            content: inserted.content,
        })
    }

    pub(super) async fn store_body(
        &mut self,
        body: BodyStream,
        maximum_resource_bytes: u64,
        maximum_total_bytes: u64,
    ) -> Result<StoredResourceBytes> {
        let inserted = self
            .content_store
            .insert_stream(body, maximum_resource_bytes, maximum_total_bytes)
            .await?;
        let bytes = inserted.content.read(maximum_resource_bytes).await?;
        Ok(StoredResourceBytes {
            bytes,
            content: inserted.content,
        })
    }

    pub(super) async fn commit_bytes(
        &mut self,
        bytes: Vec<u8>,
        stored_content: Option<StoredContent>,
        maximum_resource_bytes: u64,
        maximum_total_bytes: u64,
    ) -> Result<StoredContent> {
        match stored_content {
            Some(content)
                if content.bytes() == u64::try_from(bytes.len()).unwrap_or(u64::MAX)
                    && content.digest() == ContentDigest::sha256(&bytes) =>
            {
                Ok(content)
            }
            Some(_) => Err(PageKnotError::new(
                "pageknot.resource.store",
                ErrorStage::Internal,
                "stored resource content does not match the materialized bytes",
            )),
            None => self
                .content_store
                .insert_bytes(bytes, maximum_resource_bytes, maximum_total_bytes)
                .await
                .map(|inserted| inserted.content),
        }
    }
}

#[derive(Debug)]
pub(super) struct ResourceLoadContext<'a> {
    pub(super) page: &'a dyn PageSession,
    pub(super) session_id: &'a str,
    pub(super) cdp_frame_id: &'a str,
    pub(super) guard: &'a NetworkGuard,
    pub(super) request: &'a CaptureRequest,
    pub(super) materializer: &'a mut ResourceMaterializer,
    pub(super) budget: &'a mut CaptureBudget,
    pub(super) prefetched: &'a mut BTreeMap<ResourceId, Result<LoadedResource>>,
}

impl ResourceLoadContext<'_> {
    pub(super) async fn load(
        &mut self,
        url: &Url,
        role: RenderingRole,
        resource_id: ResourceId,
    ) -> Result<LoadedResourceData> {
        match url.scheme() {
            "data" => self.load_data(url, role).await,
            "http" | "https" => {
                let resource_url = url_without_fragment(url);
                let loaded = match self.prefetched.remove(&resource_id) {
                    Some(loaded) => loaded?,
                    None => {
                        validate_network_url(self.guard, &resource_url).await?;
                        self.page
                            .load_resource_in_session(
                                self.session_id,
                                self.cdp_frame_id,
                                &resource_url,
                                self.request.limits.resource_bytes,
                            )
                            .await?
                    }
                };
                if !(200..400).contains(&loaded.status) {
                    return Err(PageKnotError::new(
                        "pageknot.resource.status",
                        ErrorStage::Resource,
                        format!("resource returned HTTP status {}", loaded.status),
                    )
                    .retryable(loaded.status >= 500));
                }
                self.finish_loaded(url, role, loaded, None).await
            }
            "blob" => {
                let loaded = self.load_from_frame(url, resource_id, false).await?;
                self.finish_loaded(
                    url,
                    role,
                    loaded,
                    Some(ResourceRetrievalSource::OwningFrameRead),
                )
                .await
            }
            "file" => {
                let loaded = self.load_from_frame(url, resource_id, true).await?;
                self.finish_loaded(
                    url,
                    role,
                    loaded,
                    Some(ResourceRetrievalSource::LocalFileRead),
                )
                .await
            }
            scheme => Err(PageKnotError::new(
                "pageknot.resource.scheme",
                ErrorStage::Resource,
                format!("resource URL scheme `{scheme}` cannot be embedded"),
            )),
        }
    }

    async fn load_data(&mut self, url: &Url, role: RenderingRole) -> Result<LoadedResourceData> {
        let resource_url = url_without_fragment(url);
        let encoded_limit = self
            .request
            .limits
            .resource_bytes
            .saturating_mul(2)
            .saturating_add(1024);
        if u64::try_from(resource_url.as_str().len()).unwrap_or(u64::MAX) > encoded_limit {
            return Err(PageKnotError::new(
                "pageknot.resource.limit",
                ErrorStage::Resource,
                "data URL exceeds the configured encoded byte limit",
            ));
        }
        let parsed = DataUrl::process(resource_url.as_str()).map_err(|error| {
            PageKnotError::new(
                "pageknot.resource.data_url",
                ErrorStage::Resource,
                format!("resource data URL is malformed: {error}"),
            )
        })?;
        let media_type = parsed.mime_type().to_string();
        let (bytes, _) = parsed.decode_to_vec().map_err(|error| {
            PageKnotError::new(
                "pageknot.resource.data_url",
                ErrorStage::Resource,
                format!("resource data URL body is malformed: {error}"),
            )
        })?;
        let stored = self.store_bytes(bytes).await?;
        let received_bytes = stored.content.bytes();
        Ok(LoadedResourceData {
            bytes: stored.bytes,
            content: stored.content,
            media_type: effective_media_type(Some(&media_type), role, url),
            provenance: resource_provenance(
                ResourceRetrievalSource::InlineData,
                url,
                &[],
                None,
                received_bytes,
            ),
        })
    }

    async fn load_from_frame(
        &mut self,
        url: &Url,
        resource_id: ResourceId,
        validate_file: bool,
    ) -> Result<LoadedResource> {
        let resource_url = url_without_fragment(url);
        match self.prefetched.remove(&resource_id) {
            Some(loaded) => loaded,
            None => {
                if validate_file {
                    validate_file_resource(&resource_url, &self.request.capture.allowed_file_roots)
                        .await?;
                }
                self.page
                    .load_resource_in_session(
                        self.session_id,
                        self.cdp_frame_id,
                        &resource_url,
                        self.request.limits.resource_bytes,
                    )
                    .await
            }
        }
    }

    async fn finish_loaded(
        &mut self,
        requested_url: &Url,
        role: RenderingRole,
        loaded: LoadedResource,
        source_override: Option<ResourceRetrievalSource>,
    ) -> Result<LoadedResourceData> {
        let final_url = loaded.final_url;
        let redirects = loaded.redirects;
        let status = loaded.status;
        let source = source_override.unwrap_or(loaded.source);
        let media_type = effective_media_type(loaded.media_type.as_deref(), role, requested_url);
        let stored = self.store_body(loaded.body).await?;
        let received_bytes = stored.content.bytes();
        Ok(LoadedResourceData {
            bytes: stored.bytes,
            content: stored.content,
            media_type,
            provenance: resource_provenance(
                source,
                &final_url,
                &redirects,
                Some(status),
                received_bytes,
            ),
        })
    }

    async fn store_bytes(&mut self, bytes: Vec<u8>) -> Result<StoredResourceBytes> {
        let stored = self
            .materializer
            .store_bytes(
                bytes,
                self.request.limits.resource_bytes,
                self.request.limits.total_resource_bytes,
            )
            .await?;
        self.budget.reserve_resource_bytes(stored.content.bytes())?;
        Ok(stored)
    }

    async fn store_body(&mut self, body: BodyStream) -> Result<StoredResourceBytes> {
        let stored = self
            .materializer
            .store_body(
                body,
                self.request.limits.resource_bytes,
                self.request.limits.total_resource_bytes,
            )
            .await?;
        self.budget.reserve_resource_bytes(stored.content.bytes())?;
        Ok(stored)
    }
}

pub(super) fn is_css(media_type: &str) -> bool {
    media_type.eq_ignore_ascii_case("text/css")
}

pub(super) fn is_svg(media_type: &str) -> bool {
    media_type.eq_ignore_ascii_case("image/svg+xml")
}

pub(super) fn effective_media_type(
    media_type: Option<&str>,
    role: RenderingRole,
    url: &Url,
) -> String {
    media_type
        .and_then(valid_media_type_essence)
        .unwrap_or_else(|| inferred_media_type(role, url))
}

fn valid_media_type_essence(media_type: &str) -> Option<String> {
    let essence = media_type
        .split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();
    let (top_level, subtype) = essence.split_once('/')?;
    if top_level.is_empty()
        || subtype.is_empty()
        || subtype.contains('/')
        || !top_level.bytes().all(is_media_type_token_byte)
        || !subtype.bytes().all(is_media_type_token_byte)
    {
        return None;
    }
    Some(essence)
}

fn is_media_type_token_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric()
        || matches!(
            byte,
            b'!' | b'#' | b'$' | b'&' | b'^' | b'_' | b'.' | b'+' | b'-'
        )
}

pub(super) fn inferred_media_type(role: RenderingRole, url: &Url) -> String {
    let path = url.path().to_ascii_lowercase();
    let inferred = if path.ends_with(".css") || role == RenderingRole::Stylesheet {
        "text/css"
    } else if path.ends_with(".svg") || role == RenderingRole::Svg {
        "image/svg+xml"
    } else if path.ends_with(".png") {
        "image/png"
    } else if path.ends_with(".jpg") || path.ends_with(".jpeg") {
        "image/jpeg"
    } else if path.ends_with(".webp") {
        "image/webp"
    } else if path.ends_with(".gif") {
        "image/gif"
    } else if path.ends_with(".woff2") {
        "font/woff2"
    } else if path.ends_with(".woff") {
        "font/woff"
    } else if path.ends_with(".mp4") {
        "video/mp4"
    } else if path.ends_with(".mp3") {
        "audio/mpeg"
    } else if role == RenderingRole::Frame {
        "text/html"
    } else {
        "application/octet-stream"
    };
    inferred.to_owned()
}

pub(super) fn data_url(media_type: &str, bytes: &[u8]) -> String {
    format!(
        "data:{media_type};base64,{}",
        base64::engine::general_purpose::STANDARD.encode(bytes)
    )
}

pub(super) fn url_without_fragment(url: &Url) -> Url {
    let mut resource = url.clone();
    resource.set_fragment(None);
    resource
}

pub(super) async fn validate_network_url(guard: &NetworkGuard, url: &Url) -> Result<()> {
    guard.validate_url(url)?;
    let host = url.host_str().ok_or_else(|| {
        PageKnotError::new(
            "pageknot.navigation.host",
            ErrorStage::Navigation,
            "network URL must contain a host",
        )
    })?;
    if let Ok(address) = host.parse::<std::net::IpAddr>() {
        return guard.validate_resolved(url, [address]);
    }
    let port = url.port_or_known_default().ok_or_else(|| {
        PageKnotError::new(
            "pageknot.navigation.port",
            ErrorStage::Navigation,
            "network URL has no usable port",
        )
    })?;
    let addresses = tokio::net::lookup_host((host, port))
        .await
        .map_err(|error| {
            PageKnotError::new(
                "pageknot.navigation.dns",
                ErrorStage::Navigation,
                format!("failed to resolve navigation host: {error}"),
            )
            .retryable(true)
        })?
        .map(|address| address.ip())
        .collect::<Vec<_>>();
    guard.validate_resolved(url, addresses)
}

pub(super) async fn validate_file_resource(
    url: &Url,
    allowed_roots: &[pageknot_model::PortablePath],
) -> Result<()> {
    let path = url.to_file_path().map_err(|()| {
        PageKnotError::new(
            "pageknot.input.file_root",
            ErrorStage::Resource,
            "resource file URL cannot be converted to a native path",
        )
    })?;
    let path = tokio::fs::canonicalize(path).await.map_err(|error| {
        PageKnotError::new(
            "pageknot.input.file_root",
            ErrorStage::Resource,
            format!("resource file path cannot be resolved: {error}"),
        )
    })?;
    for root in allowed_roots {
        if let Ok(root) = tokio::fs::canonicalize(root.as_ref() as &std::path::Path).await
            && path.starts_with(root)
        {
            return Ok(());
        }
    }
    Err(PageKnotError::new(
        "pageknot.input.file_root",
        ErrorStage::Resource,
        "resource file path is outside the configured roots",
    ))
}

pub(super) fn resource_provenance(
    source: ResourceRetrievalSource,
    final_url: &Url,
    redirects: &[Url],
    status: Option<u16>,
    received_bytes: u64,
) -> ResourceProvenance {
    ResourceProvenance {
        source,
        final_url: RedactedUrl::from_url(final_url, &RedactionPolicy::default()),
        final_url_sha256: ContentDigest::sha256(final_url.as_str()),
        status,
        redirects: redirects
            .iter()
            .map(|url| RedactedUrl::from_url(url, &RedactionPolicy::default()))
            .collect(),
        received_bytes,
    }
}

pub(super) fn resource_record(
    id: ResourceId,
    frame_id: pageknot_model::FrameId,
    requested_url: &Url,
    outcome: ResourceOutcome,
    provenance: Option<ResourceProvenance>,
) -> ResourceRecord {
    ResourceRecord {
        id,
        frame_id,
        requested_url: RedactedUrl::from_url(requested_url, &RedactionPolicy::default()),
        requested_url_sha256: ContentDigest::sha256(requested_url.as_str()),
        outcome,
        provenance,
    }
}

pub(super) async fn materialize_visual_fallbacks(
    page: &dyn PageSession,
    session_id: &str,
    missing_resources: &MissingResourcePolicy,
    maximum_bytes: u64,
    observation: &mut PageObservation,
) -> Result<()> {
    if observation.visual_fallbacks.is_empty() {
        return Ok(());
    }
    let mut resolutions = Vec::with_capacity(observation.visual_fallbacks.len());
    for fallback in observation.visual_fallbacks.clone() {
        match page
            .capture_visual_fallback(session_id, &fallback, maximum_bytes)
            .await
        {
            Ok(bytes) => {
                resolutions.push((fallback.id, data_url("image/png", &bytes)));
            }
            Err(error) if *missing_resources == MissingResourcePolicy::Warn => {
                resolutions.push((
                    fallback.id,
                    pageknot_html::empty_resource_data_url(RenderingRole::Image),
                ));
                let noun = match fallback.kind {
                    VisualFallbackKind::Canvas => "canvas",
                    VisualFallbackKind::Video => "video",
                };
                observation
                    .warnings
                    .push(pageknot_protocol::CollectorWarning {
                        code: format!("pageknot.{noun}.capture_failed"),
                        message: format!("{noun} bitmap capture failed: {}", error.message),
                    });
            }
            Err(error) => return Err(error),
        }
    }
    let mut document = Document::parse(observation.html.as_bytes());
    for (id, data_url) in resolutions {
        document.resolve_visual_fallback(&id, &data_url)?;
    }
    let serialized = pageknot_document::serialize_document(&document).map_err(|error| {
        PageKnotError::new(
            "pageknot.artifact.serialize",
            ErrorStage::Transform,
            format!("visual fallback document could not be serialized: {error}"),
        )
    })?;
    observation.html = String::from_utf8(serialized).map_err(|error| {
        PageKnotError::new(
            "pageknot.artifact.serialize",
            ErrorStage::Transform,
            format!("visual fallback document is not valid UTF-8: {error}"),
        )
    })?;
    observation.visual_fallbacks.clear();
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::error::Error;

    use pageknot_document::RenderingRole;
    use pageknot_model::ContentDigest;
    use url::Url;

    use super::{ResourceMaterializer, effective_media_type, is_css, is_svg};

    #[test]
    fn valid_response_media_type_is_authoritative() -> Result<(), Box<dyn Error>> {
        let url = Url::parse("https://example.com/theme.css")?;

        let media_type = effective_media_type(
            Some("image/png; charset=binary"),
            RenderingRole::Stylesheet,
            &url,
        );

        assert_eq!(media_type, "image/png");
        assert!(!is_css(&media_type));
        assert!(!is_svg(&media_type));
        Ok(())
    }

    #[test]
    fn role_and_extension_infer_an_invalid_or_absent_media_type() -> Result<(), Box<dyn Error>> {
        let url = Url::parse("https://example.com/vector.svg")?;

        assert_eq!(
            effective_media_type(Some("///"), RenderingRole::Other, &url),
            "image/svg+xml"
        );
        assert_eq!(
            effective_media_type(None, RenderingRole::Stylesheet, &url),
            "text/css"
        );
        Ok(())
    }

    #[tokio::test]
    async fn changed_bytes_require_a_new_materialization() -> Result<(), Box<dyn Error>> {
        let mut materializer = ResourceMaterializer::new()?;
        let original = materializer
            .store_bytes(b"original".to_vec(), 64, 128)
            .await?;

        let mismatch = materializer
            .commit_bytes(b"changed".to_vec(), Some(original.content), 64, 128)
            .await;
        let changed = materializer
            .commit_bytes(b"changed".to_vec(), None, 64, 128)
            .await?;

        assert_eq!(
            mismatch.as_ref().map_err(|error| error.code.as_str()),
            Err("pageknot.resource.store")
        );
        assert_eq!(changed.digest(), ContentDigest::sha256(b"changed"));
        Ok(())
    }
}
