mod frame;
mod parse;

use html5ever::interface::tree_builder::QuirksMode;
use html5ever::tendril::StrTendril;
use html5ever::{Attribute, QualName};
use markup5ever::ns;
use pageknot_model::{ErrorStage, NodeId, PageKnotError, Result};

pub use frame::InlineFrame;

#[derive(Clone, Debug)]
pub struct Document {
    nodes: Vec<Node>,
    root: NodeId,
    quirks_mode: QuirksMode,
    parse_errors: Vec<String>,
}

impl Document {
    #[must_use]
    pub fn parse(bytes: &[u8]) -> Self {
        parse::parse(bytes)
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
}

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

#[cfg(test)]
mod tests;
