use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use std::time::Instant;

use pageknot_browser::{AttachedFrame, CollectorLimits, NetworkGuard, PageSession};
use pageknot_capture::CaptureBudget;
use pageknot_document::Document;
use pageknot_model::{
    CaptureEvent, CaptureId, CaptureRequest, CaptureStatus, CaptureTimings, CaptureWarning,
    ErrorStage, Milliseconds, PageKnotError, RedactedUrl, RedactionPolicy, ResourceRecord,
    ResourceSummary, Result, SourceSummary, ViewState,
};
use pageknot_protocol::PageObservation;
use url::Url;

use crate::capture_service::JobState;

use super::render::{FrameRenderContext, ResourceState, render_frame_document};
use super::resource_materialization::{materialize_visual_fallbacks, validate_network_url};
use super::{cancellable, transition};

pub(super) async fn capture(
    page: &dyn PageSession,
    job: &Arc<JobState>,
    capture_id: &CaptureId,
    request: &CaptureRequest,
    guard: &NetworkGuard,
) -> Result<CapturedIntermediate> {
    page.apply_credentials(&request.credentials, &request.url, guard)
        .await?;
    page.enable_network_guard(
        guard.clone(),
        request.credentials.headers.clone(),
        request.url.clone(),
    )
    .await?;
    transition(job, CaptureStatus::Navigating).await?;
    let navigation_started = Instant::now();
    let redaction = RedactionPolicy::default();
    job.emit(CaptureEvent::NavigationStarted {
        capture_id: capture_id.clone(),
        url: RedactedUrl::from_url(&request.url, &redaction),
    });
    let navigation_result = cancellable(
        job.cancellation(),
        page.navigate_page(
            &request.url,
            request.readiness.mode,
            request.limits.redirects,
            std::time::Duration::from(request.limits.duration),
        ),
    )
    .await?;
    let frame_id = navigation_result.frame_id;
    let final_url = navigation_result.final_url;
    for redirect in navigation_result.redirects {
        job.emit(CaptureEvent::NavigationRedirected {
            capture_id: capture_id.clone(),
            from: RedactedUrl::from_url(&redirect.from, &redaction),
            to: RedactedUrl::from_url(&redirect.to, &redaction),
            status: redirect.status,
        });
    }
    cancellable(job.cancellation(), validate_network_url(guard, &final_url)).await?;
    let navigation = navigation_started.elapsed();

    transition(job, CaptureStatus::Settling).await?;
    let settle_started = Instant::now();
    let readiness = cancellable(job.cancellation(), page.settle(&request.readiness)).await?;
    job.emit(CaptureEvent::ReadinessChanged {
        capture_id: capture_id.clone(),
        milestone: readiness.reason,
        elapsed: readiness.elapsed,
    });
    let settle = settle_started.elapsed();

    transition(job, CaptureStatus::Collecting).await?;
    let collection_started = Instant::now();
    let maximum_attached_frames = request.limits.frames.saturating_sub(1);
    let attached_frames = page
        .attached_frames_bounded(maximum_attached_frames)
        .await?;
    page.freeze_attached_frames().await?;
    let frame_descriptors = describe_frames(
        page,
        &frame_id,
        attached_frames,
        request.limits.frame_depth,
        request.limits.frames,
    )
    .await?;
    let mut budget = CaptureBudget::new(request.limits.clone());
    let mut collected_frames = BTreeMap::new();
    let mut collection_order = frame_descriptors.clone();
    collection_order.sort_by_key(|frame| (std::cmp::Reverse(frame.depth), frame.frame_id));
    for frame in &collection_order {
        let collected = cancellable(
            job.cancellation(),
            page.collect_frame_observation(
                &frame.session_id,
                frame.frame_id,
                capture_id,
                CollectorLimits {
                    frame_depth: frame.depth,
                    maximum_chunk_bytes: request.limits.collector_chunk_bytes,
                    maximum_frame_depth: request.limits.frame_depth,
                    maximum_frames: request.limits.frames.saturating_sub(budget.frames()),
                    maximum_nodes: request.limits.nodes.saturating_sub(budget.nodes()),
                    maximum_payload_bytes: budget.remaining_observation_bytes(),
                },
                &request.capture,
            ),
        )
        .await?;
        budget.reserve_observation_bytes(collected.encoded_bytes)?;
        let mut observation = collected.observation;
        materialize_visual_fallbacks(
            page,
            &frame.session_id,
            &request.capture.missing_resources,
            request.limits.resource_bytes,
            &mut observation,
        )
        .await?;
        budget.add_frames(observation.frames)?;
        budget.add_nodes(observation.subtree_nodes)?;
        job.emit(CaptureEvent::FrameCollected {
            capture_id: capture_id.clone(),
            frame_id: frame.frame_id,
            nodes: observation.nodes,
        });
        collected_frames.insert(frame.session_id.clone(), observation);
    }
    if request.capture.scope == pageknot_model::CaptureScope::Selection {
        let top_frame = frame_descriptors.first().ok_or_else(|| {
            PageKnotError::new(
                "pageknot.frame.collection",
                ErrorStage::Collection,
                "top frame descriptor is missing",
            )
        })?;
        let selected = collected_frames
            .get(&top_frame.session_id)
            .map(|observation| observation.selection.ranges)
            .unwrap_or_default();
        if selected == 0 {
            return Err(PageKnotError::new(
                "pageknot.selection.empty",
                ErrorStage::Collection,
                "the top-level document has no active selection",
            ));
        }
    }
    let frame_descriptors =
        retain_observed_frame_descriptors(frame_descriptors, &collected_frames)?;
    collection_order = frame_descriptors.clone();
    collection_order.sort_by_key(|frame| (std::cmp::Reverse(frame.depth), frame.frame_id));
    let collection = collection_started.elapsed();

    transition(job, CaptureStatus::ResolvingResources).await?;
    let resources_started = Instant::now();
    let frames = budget.frames();
    let next_frame_id = u64::try_from(frame_descriptors.len())
        .unwrap_or(u64::MAX)
        .saturating_add(1);
    let mut resource_state = ResourceState::new(budget, frames, next_frame_id)?;
    let mut rendered_frames = BTreeMap::<String, String>::new();
    for frame in &collection_order {
        let observation = collected_frames.get(&frame.session_id).ok_or_else(|| {
            PageKnotError::new(
                "pageknot.frame.collection",
                ErrorStage::Collection,
                "collected frame observation disappeared before transformation",
            )
        })?;
        let mut captured_cross_origin_frames = frame_descriptors
            .iter()
            .filter(|candidate| {
                candidate.parent_session_id.as_deref() == Some(frame.session_id.as_str())
            })
            .count();
        for warning in &observation.warnings {
            if warning.code == "pageknot.frame.cross_origin" && captured_cross_origin_frames > 0 {
                captured_cross_origin_frames -= 1;
                continue;
            }
            let warning = CaptureWarning {
                code: warning.code.clone(),
                message: warning.message.clone(),
                frame_id: Some(frame.frame_id),
                resource_id: None,
            };
            job.emit(CaptureEvent::Warning {
                capture_id: capture_id.clone(),
                warning: warning.clone(),
            });
            resource_state.warnings.push(warning);
        }
        let base_url = Url::parse(&observation.base_url).map_err(|error| {
            PageKnotError::new(
                "pageknot.collector.base_url",
                ErrorStage::Collection,
                format!("collector returned an invalid base URL: {error}"),
            )
        })?;
        let source = {
            let mut document = observation_document(observation)?;
            embed_child_frames(&mut document, frame, &frame_descriptors, &rendered_frames)?;
            pageknot_document::serialize_document(&document).map_err(|error| {
                PageKnotError::new(
                    "pageknot.artifact.serialize",
                    ErrorStage::Transform,
                    format!("collected frame could not be serialized: {error}"),
                )
            })?
        };
        let html = render_frame_document(
            source,
            base_url,
            frame.frame_id,
            frame.depth,
            FrameRenderContext {
                page,
                session_id: &frame.session_id,
                cdp_frame_id: &frame.cdp_frame_id,
                guard,
                request,
                job,
                capture_id,
            },
            &mut resource_state,
        )
        .await?;
        rendered_frames.insert(frame.session_id.clone(), html);
    }
    let resources_elapsed = resources_started.elapsed();
    let top_frame = frame_descriptors.first().ok_or_else(|| {
        PageKnotError::new(
            "pageknot.frame.collection",
            ErrorStage::Collection,
            "top frame descriptor is missing",
        )
    })?;
    let top_observation = collected_frames.get(&top_frame.session_id).ok_or_else(|| {
        PageKnotError::new(
            "pageknot.frame.collection",
            ErrorStage::Collection,
            "top frame observation is missing",
        )
    })?;
    let view_state = ViewState {
        scroll_x: top_observation.viewport.scroll_x.clone(),
        scroll_y: top_observation.viewport.scroll_y.clone(),
    };
    let html = rendered_frames
        .remove(&top_frame.session_id)
        .ok_or_else(|| {
            PageKnotError::new(
                "pageknot.frame.collection",
                ErrorStage::Transform,
                "top frame output is missing",
            )
        })?
        .into_bytes();
    let source = SourceSummary::new(&request.url, &final_url, &redaction);
    let frames = resource_state.frames;
    resource_state.validate_complete()?;
    let resource_records = resource_state.records.into_values().collect();
    let timings = CaptureTimings {
        navigation: Milliseconds::from(navigation),
        settle: Milliseconds::from(settle),
        collection: Milliseconds::from(collection),
        resources: Milliseconds::from(resources_elapsed),
        ..CaptureTimings::default()
    };
    Ok(CapturedIntermediate {
        html,
        source,
        frames,
        resources: resource_state.summary,
        resource_records,
        warnings: resource_state.warnings,
        view_state,
        timings,
    })
}

#[derive(Debug)]
pub(super) struct CapturedIntermediate {
    pub(super) html: Vec<u8>,
    pub(super) source: SourceSummary,
    pub(super) frames: u32,
    pub(super) resources: ResourceSummary,
    pub(super) resource_records: Vec<ResourceRecord>,
    pub(super) warnings: Vec<CaptureWarning>,
    pub(super) view_state: ViewState,
    pub(super) timings: CaptureTimings,
}

#[derive(Clone, Debug)]
struct FrameDescriptor {
    frame_id: pageknot_model::FrameId,
    session_id: String,
    cdp_frame_id: String,
    parent_session_id: Option<String>,
    owner_path: Option<Vec<u32>>,
    depth: u16,
}

#[derive(Debug)]
struct PendingFrame {
    frame: AttachedFrame,
    owner_path: Vec<u32>,
}

async fn describe_frames(
    page: &dyn PageSession,
    top_cdp_frame_id: &str,
    attached_frames: Vec<AttachedFrame>,
    maximum_depth: u16,
    maximum_frames: u32,
) -> Result<Vec<FrameDescriptor>> {
    let main_session = page.session_id().to_owned();
    let maximum_attached_frames = maximum_frames.saturating_sub(1);
    if u64::try_from(attached_frames.len()).unwrap_or(u64::MAX) > u64::from(maximum_attached_frames)
    {
        return Err(frame_limit_error(maximum_frames));
    }
    let mut pending = Vec::with_capacity(attached_frames.len());
    for frame in attached_frames {
        let owner_path = page.frame_owner_path(&frame).await?;
        if owner_path.is_empty() {
            return Err(PageKnotError::new(
                "pageknot.frame.owner",
                ErrorStage::Collection,
                "frame owner path is empty",
            ));
        }
        pending.push(PendingFrame { frame, owner_path });
    }
    let mut descriptors = vec![FrameDescriptor {
        frame_id: pageknot_model::FrameId::new(1),
        session_id: main_session.clone(),
        cdp_frame_id: top_cdp_frame_id.to_owned(),
        parent_session_id: None,
        owner_path: None,
        depth: 0,
    }];
    let mut known = BTreeMap::from([(main_session, (pageknot_model::FrameId::new(1), 0_u16))]);
    let mut next_id = 2_u64;
    while !pending.is_empty() {
        let (mut ready, waiting): (Vec<_>, Vec<_>) = pending
            .into_iter()
            .partition(|candidate| known.contains_key(&candidate.frame.parent_session_id));
        if ready.is_empty() {
            return Err(PageKnotError::new(
                "pageknot.frame.topology",
                ErrorStage::Collection,
                "attached frame topology is disconnected from the top frame",
            ));
        }
        ready.sort_by(|left, right| {
            let left_parent = known
                .get(&left.frame.parent_session_id)
                .map(|(id, _)| id.get())
                .unwrap_or(u64::MAX);
            let right_parent = known
                .get(&right.frame.parent_session_id)
                .map(|(id, _)| id.get())
                .unwrap_or(u64::MAX);
            (left_parent, &left.owner_path, left.frame.url.as_str()).cmp(&(
                right_parent,
                &right.owner_path,
                right.frame.url.as_str(),
            ))
        });
        for candidate in ready {
            let (_, parent_depth) = known
                .get(&candidate.frame.parent_session_id)
                .copied()
                .ok_or_else(|| {
                    PageKnotError::new(
                        "pageknot.frame.topology",
                        ErrorStage::Collection,
                        "attached frame parent disappeared during ordering",
                    )
                })?;
            let depth = child_frame_depth(parent_depth, &candidate.owner_path)?;
            if depth > maximum_depth {
                return Err(PageKnotError::new(
                    "pageknot.frame.depth",
                    ErrorStage::Collection,
                    "frame depth exceeds the configured limit",
                )
                .with_detail("depth", depth)
                .with_detail("limit", maximum_depth));
            }
            if u64::try_from(descriptors.len()).unwrap_or(u64::MAX) >= u64::from(maximum_frames) {
                return Err(frame_limit_error(maximum_frames));
            }
            let frame_id = pageknot_model::FrameId::new(next_id);
            next_id = next_id.checked_add(1).ok_or_else(|| {
                PageKnotError::new(
                    "pageknot.frame.limit",
                    ErrorStage::Collection,
                    "frame identifiers are exhausted",
                )
            })?;
            known.insert(candidate.frame.session_id.clone(), (frame_id, depth));
            descriptors.push(FrameDescriptor {
                frame_id,
                session_id: candidate.frame.session_id,
                cdp_frame_id: candidate.frame.target_id,
                parent_session_id: Some(candidate.frame.parent_session_id),
                owner_path: Some(candidate.owner_path),
                depth,
            });
        }
        pending = waiting;
    }
    Ok(descriptors)
}

fn child_frame_depth(parent_depth: u16, owner_path: &[u32]) -> Result<u16> {
    let owner_depth = u16::try_from(owner_path.len()).map_err(|error| {
        PageKnotError::new(
            "pageknot.frame.depth",
            ErrorStage::Collection,
            format!("frame owner path exceeds the supported depth range: {error}"),
        )
    })?;
    parent_depth.checked_add(owner_depth).ok_or_else(|| {
        PageKnotError::new(
            "pageknot.frame.depth",
            ErrorStage::Collection,
            "frame depth exceeds the supported numeric range",
        )
    })
}

fn frame_limit_error(maximum_frames: u32) -> PageKnotError {
    PageKnotError::new(
        "pageknot.frame.limit",
        ErrorStage::Collection,
        "frame count exceeds the configured limit",
    )
    .with_detail("limit", maximum_frames)
}

fn retain_observed_frame_descriptors(
    descriptors: Vec<FrameDescriptor>,
    observations: &BTreeMap<String, PageObservation>,
) -> Result<Vec<FrameDescriptor>> {
    let Some(top) = descriptors.first().cloned() else {
        return Err(PageKnotError::new(
            "pageknot.frame.collection",
            ErrorStage::Collection,
            "top frame descriptor is missing",
        ));
    };
    let mut retained = vec![top.clone()];
    let mut retained_sessions = BTreeSet::from([top.session_id]);
    let mut candidates = descriptors.into_iter().skip(1).collect::<Vec<_>>();
    candidates.sort_by_key(|frame| (frame.depth, frame.frame_id));
    for mut candidate in candidates {
        let Some(parent_session_id) = candidate.parent_session_id.as_deref() else {
            continue;
        };
        if !retained_sessions.contains(parent_session_id) {
            continue;
        }
        let Some(original_path) = candidate.owner_path.as_deref() else {
            continue;
        };
        let retained_path = observations
            .get(parent_session_id)
            .and_then(|observation| {
                observation
                    .frame_owners
                    .iter()
                    .find(|owner| owner.original_path == original_path)
            })
            .map(|owner| owner.retained_path.clone());
        let Some(retained_path) = retained_path.filter(|path| !path.is_empty()) else {
            continue;
        };
        candidate.owner_path = Some(retained_path);
        retained_sessions.insert(candidate.session_id.clone());
        retained.push(candidate);
    }
    Ok(retained)
}

fn embed_child_frames(
    document: &mut Document,
    parent: &FrameDescriptor,
    frames: &[FrameDescriptor],
    rendered_frames: &BTreeMap<String, String>,
) -> Result<()> {
    let mut children = frames
        .iter()
        .filter(|frame| frame.parent_session_id.as_deref() == Some(&parent.session_id))
        .collect::<Vec<_>>();
    children.sort_by(|left, right| left.owner_path.cmp(&right.owner_path));
    for child in children {
        let owner_path = child.owner_path.as_deref().ok_or_else(|| {
            PageKnotError::new(
                "pageknot.frame.owner",
                ErrorStage::Transform,
                "child frame has no parent element path",
            )
        })?;
        let html = rendered_frames.get(&child.session_id).ok_or_else(|| {
            PageKnotError::new(
                "pageknot.frame.collection",
                ErrorStage::Transform,
                "child frame output is missing before parent embedding",
            )
        })?;
        document.embed_captured_frame_at_path(owner_path, html, child.frame_id)?;
    }
    Ok(())
}

fn observation_document(observation: &PageObservation) -> Result<Document> {
    let mut bytes = observation.doctype.as_bytes().to_vec();
    bytes.extend_from_slice(observation.html.as_bytes());
    let mut document = Document::parse(&bytes);
    pageknot_document::set_document_scroll_state(
        &mut document,
        &observation.viewport.scroll_x,
        &observation.viewport.scroll_y,
    )?;
    Ok(document)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use pageknot_protocol::{
        FrameOwnerObservation, ObservationViewport, PageObservation, SelectionObservation,
    };

    use super::{
        FrameDescriptor, child_frame_depth, observation_document, retain_observed_frame_descriptors,
    };

    #[test]
    fn retained_frame_descriptors_remap_nested_pruned_owner_paths() {
        let descriptor = |id, session: &str, parent: Option<&str>, owner, depth| FrameDescriptor {
            frame_id: pageknot_model::FrameId::new(id),
            session_id: session.to_owned(),
            cdp_frame_id: format!("frame-{id}"),
            parent_session_id: parent.map(str::to_owned),
            owner_path: owner,
            depth,
        };
        let descriptors = vec![
            descriptor(1, "top", None, None, 0),
            descriptor(2, "dropped", Some("top"), Some(vec![0]), 1),
            descriptor(3, "retained", Some("top"), Some(vec![1, 2]), 2),
        ];
        let observation = |frame_owners| PageObservation {
            doctype: String::new(),
            html: "<html></html>".to_owned(),
            requested_url: "https://example.com/".to_owned(),
            final_url: "https://example.com/".to_owned(),
            base_url: "https://example.com/".to_owned(),
            title: String::new(),
            encoding: "UTF-8".to_owned(),
            viewport: ObservationViewport {
                width: 1,
                height: 1,
                device_scale_factor: "1".to_owned(),
                scroll_x: "0".to_owned(),
                scroll_y: "0".to_owned(),
            },
            frames: 1,
            nodes: 1,
            subtree_nodes: 1,
            warnings: Vec::new(),
            visual_fallbacks: Vec::new(),
            selection: SelectionObservation::default(),
            frame_owners,
        };
        let observations = BTreeMap::from([
            (
                "top".to_owned(),
                observation(vec![FrameOwnerObservation {
                    original_path: vec![1, 2],
                    retained_path: vec![0, 1],
                }]),
            ),
            ("dropped".to_owned(), observation(Vec::new())),
            ("retained".to_owned(), observation(Vec::new())),
        ]);

        let retained = retain_observed_frame_descriptors(descriptors, &observations);

        assert_eq!(
            retained.as_ref().map(|frames| frames
                .iter()
                .map(|frame| (frame.session_id.as_str(), frame.owner_path.as_deref()))
                .collect::<Vec<_>>()),
            Ok(vec![
                ("top", None),
                ("retained", Some([0_u32, 1_u32].as_slice()))
            ])
        );
    }

    #[test]
    fn nested_owner_paths_contribute_each_frame_to_depth() {
        assert_eq!(child_frame_depth(3, &[0, 2, 1]), Ok(6));
    }

    #[test]
    fn observation_scroll_state_is_materialized_on_the_document_root() {
        let observation = PageObservation {
            doctype: "<!doctype html>".to_owned(),
            html: "<html><head></head><body></body></html>".to_owned(),
            requested_url: "https://example.com/".to_owned(),
            final_url: "https://example.com/".to_owned(),
            base_url: "https://example.com/".to_owned(),
            title: String::new(),
            encoding: "UTF-8".to_owned(),
            viewport: ObservationViewport {
                width: 800,
                height: 600,
                device_scale_factor: "1".to_owned(),
                scroll_x: "24.5".to_owned(),
                scroll_y: "480".to_owned(),
            },
            frames: 1,
            nodes: 3,
            subtree_nodes: 3,
            warnings: Vec::new(),
            visual_fallbacks: Vec::new(),
            selection: SelectionObservation::default(),
            frame_owners: Vec::new(),
        };

        let html = observation_document(&observation)
            .and_then(|document| {
                pageknot_document::serialize_document(&document).map_err(|error| {
                    pageknot_model::PageKnotError::new(
                        "pageknot.artifact.serialize",
                        pageknot_model::ErrorStage::Transform,
                        error.to_string(),
                    )
                })
            })
            .map(|bytes| String::from_utf8_lossy(&bytes).into_owned());

        assert!(html.is_ok_and(|html| {
            html.contains(r#"data-pageknot-scroll-x="24.5""#)
                && html.contains(r#"data-pageknot-scroll-y="480""#)
        }));
    }
}
