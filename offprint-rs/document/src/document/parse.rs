use std::borrow::Cow;
use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::rc::Rc;

use html5ever::interface::tree_builder::{ElementFlags, NodeOrText, QuirksMode, TreeSink};
use html5ever::tendril::{StrTendril, TendrilSink};
use html5ever::{Attribute, QualName, parse_document};
use offprint_model::NodeId;

use super::{Document, Node, NodeData};

pub(super) fn parse(bytes: &[u8]) -> Document {
    parse_document(Sink::new(), Default::default())
        .from_utf8()
        .one(bytes)
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
