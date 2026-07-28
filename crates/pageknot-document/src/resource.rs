use pageknot_model::{
    ErrorStage, FrameId, NodeId, PageKnotError, ResourceId, ResourceOutcome, Result,
};
use url::Url;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ResourceLocationKind {
    HtmlAttribute,
    SrcsetCandidate,
    StyleAttribute,
    StyleElement,
    CssUrl,
    CssImageSet,
    CssFontSource,
    CssImport,
    CssCursor,
    NestedDocument,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RenderingRole {
    Image,
    Font,
    Stylesheet,
    Media,
    Frame,
    Svg,
    Cursor,
    Other,
}

#[derive(Clone, Debug)]
pub struct ResourceReference {
    pub frame_id: FrameId,
    pub node_id: NodeId,
    pub location: ResourceLocationKind,
    pub original: String,
    pub base_url: Url,
    pub resolved_url: Url,
    pub role: RenderingRole,
}

#[derive(Clone, Debug)]
pub struct ResourceGraphRecord {
    pub id: ResourceId,
    pub reference: ResourceReference,
    pub outcome: Option<ResourceOutcome>,
}

#[derive(Clone, Debug, Default)]
pub struct ResourceGraph {
    records: Vec<ResourceGraphRecord>,
}

impl ResourceGraph {
    pub fn discover(&mut self, reference: ResourceReference) -> Result<ResourceId> {
        let index = u32::try_from(self.records.len()).map_err(|error| {
            PageKnotError::new(
                "pageknot.resource.limit",
                ErrorStage::Resource,
                "resource graph exceeds the identifier range",
            )
            .with_detail("reason", error.to_string())
        })?;
        let id = ResourceId::new(index);
        self.records.push(ResourceGraphRecord {
            id,
            reference,
            outcome: None,
        });
        Ok(id)
    }

    pub fn resolve(&mut self, id: ResourceId, outcome: ResourceOutcome) -> Result<()> {
        let record = self.records.get_mut(id.get() as usize).ok_or_else(|| {
            PageKnotError::new(
                "pageknot.resource.identifier",
                ErrorStage::Resource,
                format!("resource {} is absent from the graph", id.get()),
            )
        })?;
        if record.outcome.is_some() {
            return Err(PageKnotError::new(
                "pageknot.resource.terminal",
                ErrorStage::Resource,
                format!("resource {} already has an outcome", id.get()),
            ));
        }
        record.outcome = Some(outcome);
        Ok(())
    }

    #[must_use]
    pub fn records(&self) -> &[ResourceGraphRecord] {
        &self.records
    }

    pub fn validate_complete(&self) -> Result<()> {
        let unresolved = self
            .records
            .iter()
            .filter(|record| record.outcome.is_none())
            .map(|record| record.id.get())
            .collect::<Vec<_>>();
        if unresolved.is_empty() {
            Ok(())
        } else {
            Err(PageKnotError::new(
                "pageknot.resource.incomplete",
                ErrorStage::Resource,
                "every resource reference must have a terminal outcome",
            )
            .with_detail(
                "resourceIds",
                unresolved
                    .into_iter()
                    .map(serde_json::Value::from)
                    .collect::<Vec<_>>(),
            ))
        }
    }
}
