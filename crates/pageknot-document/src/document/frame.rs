use html5ever::{Attribute, QualName};
use markup5ever::{local_name, ns};
use pageknot_model::{ErrorStage, FrameId, NodeId, PageKnotError, Result};

use super::{Document, NodeData};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InlineFrame {
    pub node_id: NodeId,
    pub html: String,
    pub base_url: Option<String>,
    pub captured: bool,
}

impl Document {
    pub fn embed_frame(&mut self, index: usize, html: &str) -> Result<NodeId> {
        self.embed_frame_with_id(index, html, None)
    }

    pub fn embed_captured_frame(
        &mut self,
        index: usize,
        html: &str,
        frame_id: FrameId,
    ) -> Result<NodeId> {
        self.embed_frame_with_id(index, html, Some(frame_id))
    }

    pub fn embed_captured_frame_at_path(
        &mut self,
        path: &[u32],
        html: &str,
        frame_id: FrameId,
    ) -> Result<()> {
        let (index, nested_path) = path.split_first().ok_or_else(|| {
            PageKnotError::new(
                "pageknot.frame.owner",
                ErrorStage::Transform,
                "captured frame owner path is empty",
            )
        })?;
        let index = usize::try_from(*index).map_err(|error| {
            PageKnotError::new(
                "pageknot.frame.owner",
                ErrorStage::Transform,
                format!("frame owner index exceeds the platform range: {error}"),
            )
        })?;
        if nested_path.is_empty() {
            self.embed_frame_with_id(index, html, Some(frame_id))?;
            return Ok(());
        }
        let owner = self.frame_owner(index)?;
        let srcdoc = self
            .node(owner)
            .and_then(|node| match &node.data {
                NodeData::Element { attrs, .. } => attrs.iter().find(|attribute| {
                    attribute.name.ns == ns!() && attribute.name.local.as_ref() == "srcdoc"
                }),
                _ => None,
            })
            .map(|attribute| attribute.value.to_string())
            .ok_or_else(|| {
                PageKnotError::new(
                    "pageknot.frame.owner",
                    ErrorStage::Transform,
                    "captured frame owner path enters a frame with no embedded document",
                )
            })?;
        let mut child = Self::parse(srcdoc.as_bytes());
        child.embed_captured_frame_at_path(nested_path, html, frame_id)?;
        let child = crate::serialize_document(&child).map_err(|error| {
            PageKnotError::new(
                "pageknot.artifact.serialize",
                ErrorStage::Transform,
                format!("nested frame document could not be serialized: {error}"),
            )
        })?;
        let child = String::from_utf8(child).map_err(|error| {
            PageKnotError::new(
                "pageknot.artifact.serialize",
                ErrorStage::Transform,
                format!("nested frame document is not UTF-8: {error}"),
            )
        })?;
        let Some(NodeData::Element { attrs, .. }) = self.node_mut(owner).map(|node| &mut node.data)
        else {
            return Err(PageKnotError::new(
                "pageknot.frame.owner",
                ErrorStage::Transform,
                "captured frame owner is not an HTML element",
            ));
        };
        let srcdoc = attrs
            .iter_mut()
            .find(|attribute| {
                attribute.name.ns == ns!() && attribute.name.local.as_ref() == "srcdoc"
            })
            .ok_or_else(|| {
                PageKnotError::new(
                    "pageknot.frame.owner",
                    ErrorStage::Transform,
                    "captured frame owner path changed during embedding",
                )
            })?;
        srcdoc.value = child.into();
        Ok(())
    }

    fn embed_frame_with_id(
        &mut self,
        index: usize,
        html: &str,
        frame_id: Option<FrameId>,
    ) -> Result<NodeId> {
        let owner = self.frame_owner(index)?;
        let Some(NodeData::Element { attrs, .. }) = self.node_mut(owner).map(|node| &mut node.data)
        else {
            return Err(PageKnotError::new(
                "pageknot.frame.owner",
                ErrorStage::Transform,
                "captured frame owner is not an HTML element",
            ));
        };
        attrs.retain(|attribute| {
            attribute.name.ns != ns!() || !matches!(attribute.name.local.as_ref(), "src" | "srcdoc")
        });
        attrs.push(Attribute {
            name: QualName::new(None, ns!(), local_name!("srcdoc")),
            value: html.into(),
        });
        if let Some(frame_id) = frame_id {
            attrs.push(Attribute {
                name: QualName::new(None, ns!(), "data-pageknot-frame-id".into()),
                value: frame_id.get().to_string().into(),
            });
        }
        Ok(owner)
    }

    fn frame_owner(&self, index: usize) -> Result<NodeId> {
        let mut stack = vec![self.root];
        let mut seen = 0;
        loop {
            let Some(id) = stack.pop() else {
                return Err(PageKnotError::new(
                    "pageknot.frame.owner",
                    ErrorStage::Transform,
                    "captured frame owner is absent from the parent document",
                )
                .with_detail("frameIndex", index));
            };
            let Some(node) = self.node(id) else {
                continue;
            };
            stack.extend(node.children.iter().rev().copied());
            if matches!(
                &node.data,
                NodeData::Element { name, .. }
                    if name.ns == ns!(html)
                        && matches!(name.local.as_ref(), "iframe" | "frame")
            ) {
                if seen == index {
                    return Ok(id);
                }
                seen += 1;
            }
        }
    }

    #[must_use]
    pub fn inline_frames(&self) -> Vec<InlineFrame> {
        self.walk()
            .filter_map(|node_id| {
                let NodeData::Element { name, attrs, .. } = &self.node(node_id)?.data else {
                    return None;
                };
                if name.ns != ns!(html) || !matches!(name.local.as_ref(), "iframe" | "frame") {
                    return None;
                }
                let html = attrs
                    .iter()
                    .find(|attribute| {
                        attribute.name.ns == ns!() && attribute.name.local.as_ref() == "srcdoc"
                    })?
                    .value
                    .to_string();
                let base_url = attrs
                    .iter()
                    .find(|attribute| {
                        attribute.name.ns == ns!()
                            && attribute.name.local.as_ref() == "data-pageknot-frame-base"
                    })
                    .map(|attribute| attribute.value.to_string());
                let captured = attrs.iter().any(|attribute| {
                    attribute.name.ns == ns!()
                        && attribute.name.local.as_ref() == "data-pageknot-frame-id"
                });
                Some(InlineFrame {
                    node_id,
                    html,
                    base_url,
                    captured,
                })
            })
            .collect()
    }

    pub fn replace_inline_frame(
        &mut self,
        frame: &InlineFrame,
        html: &str,
        frame_id: Option<FrameId>,
    ) -> Result<()> {
        let Some(NodeData::Element { attrs, .. }) =
            self.node_mut(frame.node_id).map(|node| &mut node.data)
        else {
            return Err(PageKnotError::new(
                "pageknot.frame.owner",
                ErrorStage::Transform,
                "inline frame owner is absent from the document",
            ));
        };
        let srcdoc = attrs
            .iter_mut()
            .find(|attribute| {
                attribute.name.ns == ns!() && attribute.name.local.as_ref() == "srcdoc"
            })
            .ok_or_else(|| {
                PageKnotError::new(
                    "pageknot.frame.owner",
                    ErrorStage::Transform,
                    "inline frame owner has no srcdoc attribute",
                )
            })?;
        srcdoc.value = html.into();
        attrs.retain(|attribute| {
            attribute.name.ns != ns!()
                || attribute.name.local.as_ref() != "data-pageknot-frame-base"
        });
        if let Some(frame_id) = frame_id {
            attrs.retain(|attribute| {
                attribute.name.ns != ns!()
                    || attribute.name.local.as_ref() != "data-pageknot-frame-id"
            });
            attrs.push(Attribute {
                name: QualName::new(None, ns!(), "data-pageknot-frame-id".into()),
                value: frame_id.get().to_string().into(),
            });
        }
        Ok(())
    }

    pub fn resolve_visual_fallback(&mut self, id: &str, data_url: &str) -> Result<()> {
        if self.resolve_visual_fallback_at_depth(id, data_url, 0)? {
            Ok(())
        } else {
            Err(PageKnotError::new(
                "pageknot.transform.visual_fallback",
                ErrorStage::Transform,
                "visual fallback marker is absent from the collected document",
            )
            .with_detail("fallbackId", id))
        }
    }

    fn resolve_visual_fallback_at_depth(
        &mut self,
        id: &str,
        data_url: &str,
        depth: u16,
    ) -> Result<bool> {
        if depth > 64 {
            return Err(PageKnotError::new(
                "pageknot.frame.depth",
                ErrorStage::Transform,
                "inline frame depth exceeds the visual fallback limit",
            ));
        }
        for node in &mut self.nodes {
            let NodeData::Element { attrs, .. } = &mut node.data else {
                continue;
            };
            if !attrs.iter().any(|attribute| {
                attribute.name.ns == ns!()
                    && attribute.name.local.as_ref() == "data-pageknot-visual-fallback"
                    && attribute.value.as_ref() == id
            }) {
                continue;
            }
            attrs.retain(|attribute| {
                attribute.name.ns != ns!()
                    || !matches!(
                        attribute.name.local.as_ref(),
                        "data-pageknot-visual-fallback" | "src"
                    )
            });
            attrs.push(Attribute {
                name: QualName::new(None, ns!(), local_name!("src")),
                value: data_url.into(),
            });
            return Ok(true);
        }
        for frame in self.inline_frames() {
            let mut child = Self::parse(frame.html.as_bytes());
            if !child.resolve_visual_fallback_at_depth(id, data_url, depth.saturating_add(1))? {
                continue;
            }
            let html = crate::serialize_document(&child).map_err(|error| {
                PageKnotError::new(
                    "pageknot.artifact.serialize",
                    ErrorStage::Transform,
                    format!("inline visual fallback document could not be serialized: {error}"),
                )
            })?;
            let html = String::from_utf8(html).map_err(|error| {
                PageKnotError::new(
                    "pageknot.artifact.serialize",
                    ErrorStage::Transform,
                    format!("inline visual fallback document is not UTF-8: {error}"),
                )
            })?;
            let Some(NodeData::Element { attrs, .. }) =
                self.node_mut(frame.node_id).map(|node| &mut node.data)
            else {
                return Err(PageKnotError::new(
                    "pageknot.frame.owner",
                    ErrorStage::Transform,
                    "inline visual fallback frame owner is absent",
                ));
            };
            let Some(srcdoc) = attrs.iter_mut().find(|attribute| {
                attribute.name.ns == ns!() && attribute.name.local.as_ref() == "srcdoc"
            }) else {
                return Err(PageKnotError::new(
                    "pageknot.frame.owner",
                    ErrorStage::Transform,
                    "inline visual fallback frame has no embedded document",
                ));
            };
            srcdoc.value = html.into();
            return Ok(true);
        }
        Ok(false)
    }
}
