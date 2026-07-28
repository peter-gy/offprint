use std::borrow::Cow;
use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::rc::Rc;

use html5ever::interface::tree_builder::{ElementFlags, NodeOrText, QuirksMode, TreeSink};
use html5ever::tendril::{StrTendril, TendrilSink};
use html5ever::{Attribute, QualName, parse_document};
use markup5ever::{local_name, ns};
use pageknot_model::{ErrorStage, FrameId, NodeId, PageKnotError, Result};

#[derive(Clone, Debug)]
pub struct Document {
    nodes: Vec<Node>,
    root: NodeId,
    quirks_mode: QuirksMode,
    parse_errors: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InlineFrame {
    pub node_id: NodeId,
    pub html: String,
    pub base_url: Option<String>,
    pub captured: bool,
}

impl Document {
    #[must_use]
    pub fn parse(bytes: &[u8]) -> Self {
        parse_document(Sink::new(), Default::default())
            .from_utf8()
            .one(bytes)
    }

    #[must_use]
    pub const fn root(&self) -> NodeId {
        self.root
    }

    #[must_use]
    pub fn node(&self, id: NodeId) -> Option<&Node> {
        self.nodes.get(id.get() as usize)
    }

    #[must_use]
    pub fn node_mut(&mut self, id: NodeId) -> Option<&mut Node> {
        self.nodes.get_mut(id.get() as usize)
    }

    #[must_use]
    pub fn nodes(&self) -> &[Node] {
        &self.nodes
    }

    #[must_use]
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    #[must_use]
    pub fn parse_errors(&self) -> &[String] {
        &self.parse_errors
    }

    #[must_use]
    pub const fn quirks_mode(&self) -> QuirksMode {
        self.quirks_mode
    }

    pub fn detach(&mut self, id: NodeId) {
        let parent = self.node(id).and_then(|node| node.parent);
        if let Some(parent) = parent
            && let Some(parent_node) = self.node_mut(parent)
        {
            parent_node.children.retain(|child| *child != id);
        }
        if let Some(node) = self.node_mut(id) {
            node.parent = None;
        }
    }

    pub fn create_node(&mut self, data: NodeData) -> Result<NodeId> {
        let id = u32::try_from(self.nodes.len()).map_err(|error| {
            PageKnotError::new(
                "pageknot.transform.node_limit",
                ErrorStage::Transform,
                "document exceeds the supported node identifier range",
            )
            .with_detail("reason", error.to_string())
        })?;
        let id = NodeId::new(id);
        self.nodes.push(Node {
            parent: None,
            children: Vec::new(),
            data,
        });
        Ok(id)
    }

    pub fn append_child(&mut self, parent: NodeId, child: NodeId) -> Result<()> {
        if self.node(parent).is_none() || self.node(child).is_none() {
            return Err(PageKnotError::new(
                "pageknot.transform.node",
                ErrorStage::Transform,
                "document append references an unknown node",
            ));
        }
        self.detach(child);
        if let Some(child_node) = self.node_mut(child) {
            child_node.parent = Some(parent);
        }
        if let Some(parent_node) = self.node_mut(parent) {
            parent_node.children.push(child);
        }
        Ok(())
    }

    pub fn prepend_child(&mut self, parent: NodeId, child: NodeId) -> Result<()> {
        if self.node(parent).is_none() || self.node(child).is_none() {
            return Err(PageKnotError::new(
                "pageknot.transform.node",
                ErrorStage::Transform,
                "document prepend references an unknown node",
            ));
        }
        self.detach(child);
        if let Some(child_node) = self.node_mut(child) {
            child_node.parent = Some(parent);
        }
        if let Some(parent_node) = self.node_mut(parent) {
            parent_node.children.insert(0, child);
        }
        Ok(())
    }

    #[must_use]
    pub fn find_html_element(&self, local_name: &str) -> Option<NodeId> {
        self.walk().find(|id| {
            matches!(
                self.node(*id).map(|node| &node.data),
                Some(NodeData::Element { name, .. })
                    if name.ns == html5ever::ns!(html) && name.local.as_ref() == local_name
            )
        })
    }

    #[must_use]
    pub fn find_svg_root(&self) -> Option<NodeId> {
        self.walk().find(|id| {
            matches!(
                self.node(*id).map(|node| &node.data),
                Some(NodeData::Element { name, .. })
                    if name.ns == ns!(svg) && name.local.as_ref() == "svg"
            )
        })
    }

    pub fn walk(&self) -> DocumentWalk<'_> {
        DocumentWalk {
            document: self,
            stack: vec![self.root],
        }
    }

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

pub type DocumentParse = Document;

#[derive(Clone, Debug)]
pub struct Node {
    pub parent: Option<NodeId>,
    pub children: Vec<NodeId>,
    pub data: NodeData,
}

#[derive(Clone, Debug)]
pub enum NodeData {
    Document,
    Doctype {
        name: StrTendril,
        public_id: StrTendril,
        system_id: StrTendril,
    },
    Text {
        contents: StrTendril,
    },
    Comment {
        contents: StrTendril,
    },
    Element {
        name: QualName,
        attrs: Vec<Attribute>,
        template_contents: Option<NodeId>,
        mathml_annotation_xml_integration_point: bool,
    },
    ProcessingInstruction {
        target: StrTendril,
        contents: StrTendril,
    },
}

#[derive(Debug)]
pub struct DocumentWalk<'a> {
    document: &'a Document,
    stack: Vec<NodeId>,
}

impl Iterator for DocumentWalk<'_> {
    type Item = NodeId;

    fn next(&mut self) -> Option<Self::Item> {
        let id = self.stack.pop()?;
        if let Some(node) = self.document.node(id) {
            self.stack.extend(node.children.iter().rev().copied());
            if let NodeData::Element {
                template_contents: Some(template),
                ..
            } = &node.data
            {
                self.stack.push(*template);
            }
        }
        Some(id)
    }
}

#[derive(Clone, Debug)]
struct Handle {
    id: NodeId,
    node: Rc<RawNode>,
}

#[derive(Debug)]
struct RawNode {
    parent: Cell<Option<NodeId>>,
    children: RefCell<Vec<Handle>>,
    data: RawNodeData,
}

#[derive(Debug)]
enum RawNodeData {
    Document,
    Doctype {
        name: StrTendril,
        public_id: StrTendril,
        system_id: StrTendril,
    },
    Text {
        contents: RefCell<StrTendril>,
    },
    Comment {
        contents: StrTendril,
    },
    Element {
        name: QualName,
        attrs: RefCell<Vec<Attribute>>,
        template_contents: Option<Handle>,
        mathml_annotation_xml_integration_point: bool,
    },
    ProcessingInstruction {
        target: StrTendril,
        contents: StrTendril,
    },
}

#[derive(Debug)]
struct Sink {
    nodes: RefCell<Vec<Handle>>,
    document: Handle,
    quirks_mode: Cell<QuirksMode>,
    parse_errors: RefCell<Vec<String>>,
    fallback_name: QualName,
}

impl Sink {
    fn new() -> Self {
        let root = Handle {
            id: NodeId::new(0),
            node: Rc::new(RawNode {
                parent: Cell::new(None),
                children: RefCell::new(Vec::new()),
                data: RawNodeData::Document,
            }),
        };
        Self {
            nodes: RefCell::new(vec![root.clone()]),
            document: root,
            quirks_mode: Cell::new(QuirksMode::NoQuirks),
            parse_errors: RefCell::new(Vec::new()),
            fallback_name: QualName::new(
                None,
                html5ever::ns!(html),
                html5ever::local_name!("html"),
            ),
        }
    }

    fn create_node(&self, data: RawNodeData) -> Handle {
        let mut nodes = self.nodes.borrow_mut();
        let id = NodeId::new(u32::try_from(nodes.len()).unwrap_or(u32::MAX));
        let handle = Handle {
            id,
            node: Rc::new(RawNode {
                parent: Cell::new(None),
                children: RefCell::new(Vec::new()),
                data,
            }),
        };
        nodes.push(handle.clone());
        handle
    }

    fn detach(&self, target: &Handle) {
        let Some(parent_id) = target.node.parent.take() else {
            return;
        };
        let parent = self.nodes.borrow().get(parent_id.get() as usize).cloned();
        if let Some(parent) = parent {
            parent
                .node
                .children
                .borrow_mut()
                .retain(|child| child.id != target.id);
        }
    }

    fn append_node(&self, parent: &Handle, child: Handle) {
        self.detach(&child);
        child.node.parent.set(Some(parent.id));
        parent.node.children.borrow_mut().push(child);
    }

    fn insert_before(&self, sibling: &Handle, child: Handle) {
        self.detach(&child);
        let Some(parent_id) = sibling.node.parent.get() else {
            return;
        };
        let parent = self.nodes.borrow().get(parent_id.get() as usize).cloned();
        let Some(parent) = parent else {
            return;
        };
        let mut children = parent.node.children.borrow_mut();
        let index = children
            .iter()
            .position(|candidate| candidate.id == sibling.id)
            .unwrap_or(children.len());
        child.node.parent.set(Some(parent_id));
        children.insert(index, child);
    }

    fn append_common(
        &self,
        child: NodeOrText<Handle>,
        previous: Option<Handle>,
        append: impl FnOnce(Handle),
    ) {
        let node = match child {
            NodeOrText::AppendText(text) => {
                if let Some(previous) = previous
                    && let RawNodeData::Text { contents } = &previous.node.data
                {
                    contents.borrow_mut().push_tendril(&text);
                    return;
                }
                self.create_node(RawNodeData::Text {
                    contents: RefCell::new(text),
                })
            }
            NodeOrText::AppendNode(node) => node,
        };
        append(node);
    }

    fn into_document(self) -> Document {
        let nodes = self
            .nodes
            .into_inner()
            .into_iter()
            .map(|handle| Node {
                parent: handle.node.parent.get(),
                children: handle
                    .node
                    .children
                    .borrow()
                    .iter()
                    .map(|child| child.id)
                    .collect(),
                data: raw_data_to_node_data(&handle.node.data),
            })
            .collect();
        Document {
            nodes,
            root: self.document.id,
            quirks_mode: self.quirks_mode.get(),
            parse_errors: self.parse_errors.into_inner(),
        }
    }
}

fn raw_data_to_node_data(data: &RawNodeData) -> NodeData {
    match data {
        RawNodeData::Document => NodeData::Document,
        RawNodeData::Doctype {
            name,
            public_id,
            system_id,
        } => NodeData::Doctype {
            name: name.clone(),
            public_id: public_id.clone(),
            system_id: system_id.clone(),
        },
        RawNodeData::Text { contents } => NodeData::Text {
            contents: contents.borrow().clone(),
        },
        RawNodeData::Comment { contents } => NodeData::Comment {
            contents: contents.clone(),
        },
        RawNodeData::Element {
            name,
            attrs,
            template_contents,
            mathml_annotation_xml_integration_point,
        } => NodeData::Element {
            name: name.clone(),
            attrs: attrs.borrow().clone(),
            template_contents: template_contents.as_ref().map(|handle| handle.id),
            mathml_annotation_xml_integration_point: *mathml_annotation_xml_integration_point,
        },
        RawNodeData::ProcessingInstruction { target, contents } => {
            NodeData::ProcessingInstruction {
                target: target.clone(),
                contents: contents.clone(),
            }
        }
    }
}

impl TreeSink for Sink {
    type Handle = Handle;
    type Output = Document;
    type ElemName<'a>
        = &'a QualName
    where
        Self: 'a;

    fn finish(self) -> Self::Output {
        self.into_document()
    }

    fn parse_error(&self, message: Cow<'static, str>) {
        self.parse_errors.borrow_mut().push(message.into_owned());
    }

    fn get_document(&self) -> Self::Handle {
        self.document.clone()
    }

    fn set_quirks_mode(&self, mode: QuirksMode) {
        self.quirks_mode.set(mode);
    }

    fn same_node(&self, left: &Self::Handle, right: &Self::Handle) -> bool {
        left.id == right.id
    }

    fn elem_name<'a>(&'a self, target: &'a Self::Handle) -> Self::ElemName<'a> {
        if let RawNodeData::Element { name, .. } = &target.node.data {
            name
        } else {
            &self.fallback_name
        }
    }

    fn get_template_contents(&self, target: &Self::Handle) -> Self::Handle {
        if let RawNodeData::Element {
            template_contents: Some(contents),
            ..
        } = &target.node.data
        {
            contents.clone()
        } else {
            self.document.clone()
        }
    }

    fn is_mathml_annotation_xml_integration_point(&self, target: &Self::Handle) -> bool {
        matches!(
            &target.node.data,
            RawNodeData::Element {
                mathml_annotation_xml_integration_point: true,
                ..
            }
        )
    }

    fn create_element(
        &self,
        name: QualName,
        attrs: Vec<Attribute>,
        flags: ElementFlags,
    ) -> Self::Handle {
        let template_contents = flags
            .template
            .then(|| self.create_node(RawNodeData::Document));
        self.create_node(RawNodeData::Element {
            name,
            attrs: RefCell::new(attrs),
            template_contents,
            mathml_annotation_xml_integration_point: flags.mathml_annotation_xml_integration_point,
        })
    }

    fn create_comment(&self, text: StrTendril) -> Self::Handle {
        self.create_node(RawNodeData::Comment { contents: text })
    }

    fn create_pi(&self, target: StrTendril, data: StrTendril) -> Self::Handle {
        self.create_node(RawNodeData::ProcessingInstruction {
            target,
            contents: data,
        })
    }

    fn append(&self, parent: &Self::Handle, child: NodeOrText<Self::Handle>) {
        let previous = parent.node.children.borrow().last().cloned();
        self.append_common(child, previous, |node| self.append_node(parent, node));
    }

    fn append_before_sibling(&self, sibling: &Self::Handle, child: NodeOrText<Self::Handle>) {
        let previous = sibling.node.parent.get().and_then(|parent_id| {
            self.nodes
                .borrow()
                .get(parent_id.get() as usize)
                .and_then(|parent| {
                    let children = parent.node.children.borrow();
                    let index = children
                        .iter()
                        .position(|candidate| candidate.id == sibling.id)?;
                    index
                        .checked_sub(1)
                        .and_then(|index| children.get(index).cloned())
                })
        });
        self.append_common(child, previous, |node| self.insert_before(sibling, node));
    }

    fn append_based_on_parent_node(
        &self,
        element: &Self::Handle,
        previous_element: &Self::Handle,
        child: NodeOrText<Self::Handle>,
    ) {
        if element.node.parent.get().is_some() {
            self.append_before_sibling(element, child);
        } else {
            self.append(previous_element, child);
        }
    }

    fn append_doctype_to_document(
        &self,
        name: StrTendril,
        public_id: StrTendril,
        system_id: StrTendril,
    ) {
        let doctype = self.create_node(RawNodeData::Doctype {
            name,
            public_id,
            system_id,
        });
        self.append_node(&self.document, doctype);
    }

    fn add_attrs_if_missing(&self, target: &Self::Handle, attrs: Vec<Attribute>) {
        let RawNodeData::Element {
            attrs: existing, ..
        } = &target.node.data
        else {
            return;
        };
        let names = existing
            .borrow()
            .iter()
            .map(|attribute| attribute.name.clone())
            .collect::<HashSet<_>>();
        existing.borrow_mut().extend(
            attrs
                .into_iter()
                .filter(|attribute| !names.contains(&attribute.name)),
        );
    }

    fn remove_from_parent(&self, target: &Self::Handle) {
        self.detach(target);
    }

    fn reparent_children(&self, node: &Self::Handle, new_parent: &Self::Handle) {
        let children = std::mem::take(&mut *node.node.children.borrow_mut());
        for child in children {
            child.node.parent.set(None);
            self.append_node(new_parent, child);
        }
    }
}

#[cfg(test)]
mod tests;
