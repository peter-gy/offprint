use std::collections::{BTreeMap, BTreeSet};
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use futures_util::StreamExt as _;
use pageknot_browser::{LoadedResource, NetworkGuard, PageSession};
use pageknot_capture::{CaptureBudget, StoredContent};
use pageknot_document::{
    CssParseMode, Document, RenderingRole, discover_css_resources_bounded,
    discover_document_resources_bounded, serialize_document,
};
use pageknot_model::{
    CaptureEvent, CaptureId, CaptureRequest, CaptureWarning, ContentDigest, ErrorStage,
    MissingResourcePolicy, PageKnotError, ResourceError, ResourceId, ResourceOutcome,
    ResourceProvenance, ResourceRecord, ResourceSummary, Result,
};
use url::Url;

use crate::capture_service::JobState;

use super::check_cancelled;
use super::resource_materialization::{
    LoadedResourceData, ResourceLoadContext, ResourceMaterializer, data_url, is_css, is_svg,
    resource_record, url_without_fragment, validate_file_resource, validate_network_url,
};

#[derive(Debug)]
struct ResourceResolver<'a> {
    page: &'a dyn PageSession,
    session_id: &'a str,
    cdp_frame_id: &'a str,
    manifest_frame_id: pageknot_model::FrameId,
    guard: &'a NetworkGuard,
    request: &'a CaptureRequest,
    job: &'a Arc<JobState>,
    capture_id: &'a CaptureId,
    state: &'a mut ResourceState,
    active_css: BTreeSet<String>,
    active_svg: BTreeSet<String>,
    prefetched: BTreeMap<ResourceId, Result<LoadedResource>>,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct ResourceLoadKey {
    session_id: String,
    cdp_frame_id: String,
    url: String,
}

impl ResourceLoadKey {
    fn new(session_id: &str, cdp_frame_id: &str, url: &Url) -> Self {
        Self {
            session_id: session_id.to_owned(),
            cdp_frame_id: cdp_frame_id.to_owned(),
            url: url_without_fragment(url).to_string(),
        }
    }
}

#[derive(Debug)]
pub(super) struct ResourceState {
    budget: CaptureBudget,
    materializer: ResourceMaterializer,
    pub(super) summary: ResourceSummary,
    pub(super) warnings: Vec<CaptureWarning>,
    pub(super) records: BTreeMap<ResourceId, ResourceRecord>,
    failed_loads: BTreeMap<ResourceLoadKey, PageKnotError>,
    embedded_digests: BTreeSet<ContentDigest>,
    next_resource_id: u32,
    pub(super) frames: u32,
    next_frame_id: u64,
}

impl ResourceState {
    pub(super) fn new(budget: CaptureBudget, frames: u32, next_frame_id: u64) -> Result<Self> {
        Ok(Self {
            budget,
            materializer: ResourceMaterializer::new()?,
            summary: ResourceSummary::default(),
            warnings: Vec::new(),
            records: BTreeMap::new(),
            failed_loads: BTreeMap::new(),
            embedded_digests: BTreeSet::new(),
            next_resource_id: 0,
            frames,
            next_frame_id,
        })
    }

    fn begin_inline_frame(
        &mut self,
        document: &Document,
    ) -> Result<(pageknot_model::FrameId, u64)> {
        let nodes = u64::try_from(document.node_count()).map_err(|error| {
            PageKnotError::new(
                "pageknot.frame.nodes",
                ErrorStage::Collection,
                format!("inline frame node count exceeds the supported range: {error}"),
            )
        })?;
        let frame_id = pageknot_model::FrameId::new(self.next_frame_id);
        self.next_frame_id = self.next_frame_id.checked_add(1).ok_or_else(|| {
            PageKnotError::new(
                "pageknot.frame.limit",
                ErrorStage::Collection,
                "frame identifiers are exhausted",
            )
        })?;
        Ok((frame_id, nodes))
    }

    fn begin_reference(&mut self) -> Result<ResourceId> {
        self.budget.add_resource()?;
        self.summary.discovered = self.summary.discovered.saturating_add(1);
        let id = ResourceId::new(self.next_resource_id);
        self.next_resource_id = self.next_resource_id.checked_add(1).ok_or_else(|| {
            PageKnotError::new(
                "pageknot.resource.limit",
                ErrorStage::Resource,
                "resource identifiers are exhausted",
            )
        })?;
        Ok(id)
    }

    fn remaining_resources(&self, maximum: u32) -> Result<usize> {
        let remaining = maximum
            .checked_sub(self.budget.resources())
            .ok_or_else(|| {
                PageKnotError::new(
                    "pageknot.resource.limit",
                    ErrorStage::Resource,
                    "resource count exceeds the configured limit",
                )
                .with_detail("attempted", self.budget.resources())
                .with_detail("limit", maximum)
            })?;
        usize::try_from(remaining).map_err(|error| {
            PageKnotError::new(
                "pageknot.resource.limit",
                ErrorStage::Resource,
                format!("remaining resource capacity exceeds the platform range: {error}"),
            )
        })
    }

    fn record(&mut self, record: ResourceRecord) -> Result<()> {
        let id = record.id;
        if self.records.insert(id, record).is_some() {
            return Err(PageKnotError::new(
                "pageknot.resource.terminal",
                ErrorStage::Resource,
                format!("resource {} already has a terminal outcome", id.get()),
            ));
        }
        Ok(())
    }

    pub(super) fn validate_complete(&self) -> Result<()> {
        let record_count = u32::try_from(self.records.len()).map_err(|error| {
            PageKnotError::new(
                "pageknot.resource.incomplete",
                ErrorStage::Resource,
                format!("resource record count exceeds the supported range: {error}"),
            )
        })?;
        if record_count == self.summary.discovered && self.summary.is_complete() {
            Ok(())
        } else {
            Err(PageKnotError::new(
                "pageknot.resource.incomplete",
                ErrorStage::Resource,
                "every discovered resource must have one terminal outcome",
            )
            .with_detail("discovered", self.summary.discovered)
            .with_detail("records", record_count)
            .with_detail("outcomes", self.summary.outcomes()))
        }
    }
}

#[derive(Debug)]
struct InlineFrameReplacement {
    html: String,
    frame_id: pageknot_model::FrameId,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct FrameRenderContext<'a> {
    pub(super) page: &'a dyn PageSession,
    pub(super) session_id: &'a str,
    pub(super) cdp_frame_id: &'a str,
    pub(super) guard: &'a NetworkGuard,
    pub(super) request: &'a CaptureRequest,
    pub(super) job: &'a Arc<JobState>,
    pub(super) capture_id: &'a CaptureId,
}

async fn prefetch_resources(
    context: FrameRenderContext<'_>,
    batch: &[(&pageknot_document::DiscoveredDocumentResource, ResourceId)],
    failed_loads: &BTreeMap<ResourceLoadKey, PageKnotError>,
) -> BTreeMap<ResourceId, Result<LoadedResource>> {
    let candidates = batch
        .iter()
        .filter_map(|(resource, resource_id)| {
            if matches!(
                resource.resolved_url.scheme(),
                "http" | "https" | "blob" | "file"
            ) {
                let url = url_without_fragment(&resource.resolved_url);
                let key = ResourceLoadKey::new(context.session_id, context.cdp_frame_id, &url);
                (!failed_loads.contains_key(&key)).then_some((*resource_id, url))
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    futures_util::stream::iter(candidates)
        .map(|(resource_id, url)| async move {
            let validation = match url.scheme() {
                "http" | "https" => validate_network_url(context.guard, &url).await,
                "file" => {
                    validate_file_resource(&url, &context.request.capture.allowed_file_roots).await
                }
                "blob" => Ok(()),
                _ => Ok(()),
            };
            let result = match validation {
                Ok(()) => {
                    context
                        .page
                        .load_resource_in_session(
                            context.session_id,
                            context.cdp_frame_id,
                            &url,
                            context.request.limits.resource_bytes,
                        )
                        .await
                }
                Err(error) => Err(error),
            };
            (resource_id, result)
        })
        .buffer_unordered(batch.len().max(1))
        .collect()
        .await
}

pub(super) fn render_frame_document<'a>(
    source: Vec<u8>,
    base_url: Url,
    manifest_frame_id: pageknot_model::FrameId,
    depth: u16,
    context: FrameRenderContext<'a>,
    state: &'a mut ResourceState,
) -> Pin<Box<dyn Future<Output = Result<String>> + Send + 'a>> {
    Box::pin(async move {
        let inline_frames = Document::parse(&source).inline_frames();
        let mut inline_replacements = BTreeMap::new();
        for frame in inline_frames.iter().filter(|frame| !frame.captured) {
            let child_depth = depth.checked_add(1).ok_or_else(|| {
                PageKnotError::new(
                    "pageknot.frame.depth",
                    ErrorStage::Collection,
                    "frame depth exceeds the supported numeric range",
                )
            })?;
            if child_depth > context.request.limits.frame_depth {
                return Err(PageKnotError::new(
                    "pageknot.frame.depth",
                    ErrorStage::Collection,
                    "frame depth exceeds the configured limit",
                )
                .with_detail("depth", child_depth)
                .with_detail("limit", context.request.limits.frame_depth));
            }
            let child_base = match &frame.base_url {
                Some(value) => Url::parse(value).map_err(|error| {
                    PageKnotError::new(
                        "pageknot.collector.base_url",
                        ErrorStage::Collection,
                        format!("collector returned an invalid frame base URL: {error}"),
                    )
                })?,
                None => base_url.clone(),
            };
            let (child_frame_id, child_nodes) =
                state.begin_inline_frame(&Document::parse(frame.html.as_bytes()))?;
            context.job.emit(CaptureEvent::FrameCollected {
                capture_id: context.capture_id.clone(),
                frame_id: child_frame_id,
                nodes: child_nodes,
            });
            let rendered = render_frame_document(
                frame.html.as_bytes().to_vec(),
                child_base,
                child_frame_id,
                child_depth,
                context,
                state,
            )
            .await?;
            inline_replacements.insert(
                frame.node_id,
                InlineFrameReplacement {
                    html: rendered,
                    frame_id: child_frame_id,
                },
            );
        }

        let resource_inputs = {
            let mut document = Document::parse(&source);
            apply_inline_frame_replacements(&mut document, &inline_frames, &inline_replacements)?;
            let remaining = state.remaining_resources(context.request.limits.resources)?;
            discover_document_resources_bounded(&document, &base_url, remaining)?
                .resources()
                .to_vec()
        };
        let mut replacements = BTreeMap::new();
        let mut references = Vec::with_capacity(resource_inputs.len());
        for resource in &resource_inputs {
            let global_id = state.begin_reference()?;
            context.job.emit(CaptureEvent::ResourceDiscovered {
                capture_id: context.capture_id.clone(),
                resource_id: global_id,
            });
            references.push((resource, global_id));
        }
        let concurrency = usize::from(context.request.limits.concurrent_resources).max(1);
        for batch in references.chunks(concurrency) {
            let prefetched = prefetch_resources(context, batch, &state.failed_loads).await;
            let mut resolver = ResourceResolver {
                page: context.page,
                session_id: context.session_id,
                cdp_frame_id: context.cdp_frame_id,
                manifest_frame_id,
                guard: context.guard,
                request: context.request,
                job: context.job,
                capture_id: context.capture_id,
                state,
                active_css: BTreeSet::new(),
                active_svg: BTreeSet::new(),
                prefetched,
            };
            for (resource, global_id) in batch {
                let replacement = resolver
                    .resolve(resource.resolved_url.clone(), resource.role, 0, *global_id)
                    .await?;
                replacements.insert(resource.id, replacement);
            }
        }

        let mut document = Document::parse(&source);
        apply_inline_frame_replacements(&mut document, &inline_frames, &inline_replacements)?;
        let discovered =
            discover_document_resources_bounded(&document, &base_url, resource_inputs.len())?;
        if discovered.resources().len() != resource_inputs.len() {
            return Err(PageKnotError::new(
                "pageknot.resource.rewrite",
                ErrorStage::Resource,
                "document resource inventory changed before rewriting",
            ));
        }
        discovered.rewrite(&mut document, &replacements)?;
        serialize_frame_document(&document)
    })
}

pub(super) fn serialize_frame_document(document: &Document) -> Result<String> {
    let html = serialize_document(document).map_err(|error| {
        PageKnotError::new(
            "pageknot.artifact.serialize",
            ErrorStage::Transform,
            format!("frame could not be serialized: {error}"),
        )
    })?;
    String::from_utf8(html).map_err(|error| {
        PageKnotError::new(
            "pageknot.artifact.serialize",
            ErrorStage::Transform,
            format!("frame is not UTF-8: {error}"),
        )
    })
}

fn apply_inline_frame_replacements(
    document: &mut Document,
    inline_frames: &[pageknot_document::InlineFrame],
    replacements: &BTreeMap<pageknot_model::NodeId, InlineFrameReplacement>,
) -> Result<()> {
    for frame in inline_frames {
        let replacement = replacements.get(&frame.node_id);
        let html = replacement
            .map(|replacement| replacement.html.as_str())
            .unwrap_or(&frame.html);
        let frame_id = replacement.map(|replacement| replacement.frame_id);
        document.replace_inline_frame(frame, html, frame_id)?;
    }
    Ok(())
}

impl ResourceResolver<'_> {
    fn begin_reference(&mut self) -> Result<ResourceId> {
        self.state.begin_reference()
    }

    fn remaining_resources(&self) -> Result<usize> {
        self.state
            .remaining_resources(self.request.limits.resources)
    }

    fn resolve<'a>(
        &'a mut self,
        url: Url,
        role: RenderingRole,
        nested_depth: u16,
        resource_id: ResourceId,
    ) -> Pin<Box<dyn Future<Output = Result<String>> + Send + 'a>> {
        Box::pin(async move {
            check_cancelled(self.job.cancellation())?;
            let load_key = ResourceLoadKey::new(self.session_id, self.cdp_frame_id, &url);
            let loaded = match self.state.failed_loads.get(&load_key) {
                Some(error) => Err(error.clone()),
                None => self.load(&url, role, resource_id).await,
            };
            let LoadedResourceData {
                mut bytes,
                content,
                media_type,
                provenance,
            } = match loaded {
                Ok(loaded) => loaded,
                Err(error) => {
                    self.state
                        .failed_loads
                        .entry(load_key)
                        .or_insert_with(|| error.clone());
                    return self.missing(url, role, resource_id, error);
                }
            };
            let mut stored_content = Some(content);
            if is_css(&media_type) {
                if nested_depth >= self.request.limits.css_import_depth {
                    let error = PageKnotError::new(
                        "pageknot.resource.css_depth",
                        ErrorStage::Resource,
                        "CSS import depth exceeds the configured limit",
                    );
                    return self.missing(url, role, resource_id, error);
                }
                let key = url_without_fragment(&url).to_string();
                if !self.active_css.insert(key.clone()) {
                    let error = PageKnotError::new(
                        "pageknot.resource.css_cycle",
                        ErrorStage::Resource,
                        "CSS imports contain a cycle",
                    );
                    return self.missing(url, role, resource_id, error);
                }
                let transformed = async {
                    let css = String::from_utf8(bytes).map_err(|error| {
                        PageKnotError::new(
                            "pageknot.resource.css_encoding",
                            ErrorStage::Resource,
                            format!("stylesheet is not UTF-8: {error}"),
                        )
                    })?;
                    let remaining = self.remaining_resources()?;
                    let css_resources = discover_css_resources_bounded(&css, &url, remaining)?;
                    if css_resources.parse_mode() == CssParseMode::PreservationFallback {
                        let warning = CaptureWarning {
                            code: "pageknot.resource.css_preservation".to_owned(),
                            message: "stylesheet uses source-preserving CSS resource discovery"
                                .to_owned(),
                            frame_id: Some(self.manifest_frame_id),
                            resource_id: Some(resource_id),
                        };
                        self.job.emit(CaptureEvent::Warning {
                            capture_id: self.capture_id.clone(),
                            warning: warning.clone(),
                        });
                        self.state.warnings.push(warning);
                    }
                    let mut replacements = BTreeMap::new();
                    for nested in css_resources.resources() {
                        let nested_id = self.begin_reference()?;
                        self.job.emit(CaptureEvent::ResourceDiscovered {
                            capture_id: self.capture_id.clone(),
                            resource_id: nested_id,
                        });
                        let replacement = self
                            .resolve(
                                nested.resolved_url.clone(),
                                RenderingRole::Other,
                                nested_depth.saturating_add(1),
                                nested_id,
                            )
                            .await?;
                        replacements.insert(nested.id, replacement);
                    }
                    css_resources.rewrite(&replacements).map(String::into_bytes)
                }
                .await;
                self.active_css.remove(&key);
                bytes = match transformed {
                    Ok(bytes) => {
                        stored_content = None;
                        bytes
                    }
                    Err(error) if error.code.as_str() == "pageknot.resource.limit" => {
                        return Err(error);
                    }
                    Err(error) => return self.missing(url, role, resource_id, error),
                };
            }
            if is_svg(&media_type) {
                if nested_depth >= self.request.limits.css_import_depth {
                    let error = PageKnotError::new(
                        "pageknot.resource.svg_depth",
                        ErrorStage::Resource,
                        "SVG resource depth exceeds the configured limit",
                    );
                    return self.missing(url, role, resource_id, error);
                }
                let key = url_without_fragment(&url).to_string();
                if !self.active_svg.insert(key.clone()) {
                    let error = PageKnotError::new(
                        "pageknot.resource.svg_cycle",
                        ErrorStage::Resource,
                        "SVG resources contain a cycle",
                    );
                    return self.missing(url, role, resource_id, error);
                }
                let transformed = async {
                    let resource_inputs = {
                        let document = Document::parse(&bytes);
                        if document.find_svg_root().is_none() {
                            return Err(PageKnotError::new(
                                "pageknot.resource.svg_parse",
                                ErrorStage::Resource,
                                "SVG resource has no root SVG element",
                            ));
                        }
                        let remaining = self.remaining_resources()?;
                        discover_document_resources_bounded(&document, &url, remaining)?
                            .resources()
                            .to_vec()
                    };
                    let mut replacements = BTreeMap::new();
                    for nested in &resource_inputs {
                        let nested_id = self.begin_reference()?;
                        self.job.emit(CaptureEvent::ResourceDiscovered {
                            capture_id: self.capture_id.clone(),
                            resource_id: nested_id,
                        });
                        let replacement = self
                            .resolve(
                                nested.resolved_url.clone(),
                                nested.role,
                                nested_depth.saturating_add(1),
                                nested_id,
                            )
                            .await?;
                        replacements.insert(nested.id, replacement);
                    }
                    let mut document = Document::parse(&bytes);
                    let svg_root = document.find_svg_root().ok_or_else(|| {
                        PageKnotError::new(
                            "pageknot.resource.svg_parse",
                            ErrorStage::Resource,
                            "SVG resource has no root SVG element",
                        )
                    })?;
                    let discovered = discover_document_resources_bounded(
                        &document,
                        &url,
                        resource_inputs.len(),
                    )?;
                    if discovered.resources().len() != resource_inputs.len() {
                        return Err(PageKnotError::new(
                            "pageknot.resource.rewrite",
                            ErrorStage::Resource,
                            "SVG resource inventory changed before rewriting",
                        ));
                    }
                    discovered.rewrite(&mut document, &replacements)?;
                    pageknot_document::serialize_subtree(&document, svg_root).map_err(|error| {
                        PageKnotError::new(
                            "pageknot.resource.svg_serialize",
                            ErrorStage::Resource,
                            format!("SVG resource could not be serialized: {error}"),
                        )
                    })
                }
                .await;
                self.active_svg.remove(&key);
                bytes = match transformed {
                    Ok(bytes) => {
                        stored_content = None;
                        bytes
                    }
                    Err(error) if error.code.as_str() == "pageknot.resource.limit" => {
                        return Err(error);
                    }
                    Err(error) => return self.missing(url, role, resource_id, error),
                };
            }
            self.record_embedded(
                url,
                resource_id,
                media_type,
                bytes,
                stored_content,
                provenance,
            )
            .await
        })
    }

    async fn load(
        &mut self,
        url: &Url,
        role: RenderingRole,
        resource_id: ResourceId,
    ) -> Result<LoadedResourceData> {
        ResourceLoadContext {
            page: self.page,
            session_id: self.session_id,
            cdp_frame_id: self.cdp_frame_id,
            guard: self.guard,
            request: self.request,
            materializer: &mut self.state.materializer,
            budget: &mut self.state.budget,
            prefetched: &mut self.prefetched,
        }
        .load(url, role, resource_id)
        .await
    }

    async fn record_embedded(
        &mut self,
        url: Url,
        resource_id: ResourceId,
        media_type: String,
        bytes: Vec<u8>,
        stored_content: Option<StoredContent>,
        provenance: ResourceProvenance,
    ) -> Result<String> {
        let mut replacement = data_url(&media_type, &bytes);
        let content = self
            .state
            .materializer
            .commit_bytes(
                bytes,
                stored_content,
                self.request.limits.resource_bytes,
                self.request.limits.total_resource_bytes,
            )
            .await?;
        self.state.summary.embedded = self.state.summary.embedded.saturating_add(1);
        if self.state.embedded_digests.insert(content.digest()) {
            self.state.summary.embedded_bytes = self
                .state
                .summary
                .embedded_bytes
                .saturating_add(content.bytes());
        }
        self.state.record(resource_record(
            resource_id,
            self.manifest_frame_id,
            &url,
            ResourceOutcome::Embedded {
                digest: content.digest(),
                media_type: media_type.clone(),
                bytes: content.bytes(),
            },
            Some(provenance),
        ))?;
        self.progress();
        if let Some(fragment) = url.fragment() {
            replacement.push('#');
            replacement.push_str(fragment);
        }
        Ok(replacement)
    }

    fn missing(
        &mut self,
        url: Url,
        role: RenderingRole,
        resource_id: ResourceId,
        error: PageKnotError,
    ) -> Result<String> {
        let outcome = ResourceOutcome::Failed {
            error: ResourceError {
                code: error.code.clone(),
                message: error.message.clone(),
                retryable: error.retryable,
            },
        };
        self.state.record(resource_record(
            resource_id,
            self.manifest_frame_id,
            &url,
            outcome,
            None,
        ))?;
        self.state.summary.failed = self.state.summary.failed.saturating_add(1);
        self.progress();
        if self.request.capture.missing_resources == MissingResourcePolicy::Fail {
            return Err(error);
        }
        let warning = CaptureWarning {
            code: error.code.to_string(),
            message: error.message,
            frame_id: Some(self.manifest_frame_id),
            resource_id: Some(resource_id),
        };
        self.job.emit(CaptureEvent::Warning {
            capture_id: self.capture_id.clone(),
            warning: warning.clone(),
        });
        self.state.warnings.push(warning);
        Ok(pageknot_html::empty_resource_data_url(role))
    }

    fn progress(&self) {
        self.job.emit(CaptureEvent::ResourceProgress {
            capture_id: self.capture_id.clone(),
            completed: self.state.summary.outcomes(),
            discovered: self.state.summary.discovered,
            bytes: self.state.summary.embedded_bytes,
        });
    }
}

#[cfg(test)]
mod tests {
    use pageknot_capture::CaptureBudget;
    use pageknot_model::CaptureLimits;

    use super::ResourceState;

    #[test]
    fn resource_capacity_fails_before_growing_the_inventory() -> pageknot_model::Result<()> {
        let limits = CaptureLimits {
            resources: 1,
            ..CaptureLimits::default()
        };
        let mut state = ResourceState::new(CaptureBudget::new(limits), 1, 2)?;

        assert!(state.begin_reference().is_ok());
        let result = state.begin_reference();

        assert_eq!(
            result.as_ref().map_err(|error| error.code.as_str()),
            Err("pageknot.resource.limit")
        );
        assert_eq!(state.summary.discovered, 1);
        assert_eq!(state.budget.resources(), 1);
        Ok(())
    }
}
